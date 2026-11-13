// The bus over the shared engine: pgmq on PostgreSQL.
//
// Same interface as the local bus, so delivery does not learn which engine it is talking to. The
// differences are all below this line: a queue per address, a cursor in `msgq.cursor`, and a wake
// from the engine's insert notification with polling underneath.
//
// `pg` is an optional dependency: a session on a local file never loads it, and a session pointed
// at a server that cannot load it reports that rather than pretending to be connected.

import type { Bus, BusEvent, Subscription } from "./consumer.ts";

/** Records fetched per read. */
const READ_BATCH = 100;

/** Fallback wait when nothing notifies. */
const IDLE_WAIT_MS = 1000;

interface PgResult {
	rows: Array<Record<string, unknown>>;
	rowCount: number | null;
}

interface PgClient {
	query(text: string, values?: unknown[]): Promise<PgResult>;
	on(event: "notification", handler: (message: { channel: string }) => void): void;
	off(event: "notification", handler: (message: { channel: string }) => void): void;
	end(): Promise<void>;
}

interface PgModule {
	Client: new (config: Record<string, unknown>) => PgClient;
}

/**
 * Connection settings for the shared engine.
 *
 * The same names the CLI reads, with the same precedence: what the location string says wins, and
 * the `PI_MSGQ_PG*` variables supply what it does not.
 */
export function pgSettings(
	location: string,
	env: Record<string, string | undefined>,
): Record<string, unknown> {
	const settings: Record<string, unknown> = {};
	if (location.includes("://")) settings.connectionString = location;
	const map: Array<[string, string]> = [
		["PI_MSGQ_PGHOST", "host"],
		["PI_MSGQ_PGPORT", "port"],
		["PI_MSGQ_PGUSER", "user"],
		["PI_MSGQ_PGPASSWORD", "password"],
		["PI_MSGQ_PGDATABASE", "database"],
	];
	for (const [variable, key] of map) {
		const value = env[variable]?.trim();
		if (!value) continue;
		// A setting already present in the location is not overwritten, which is how libpq treats
		// its environment: defaults, not overrides.
		if (typeof settings[key] !== "undefined") continue;
		settings[key] = key === "port" ? Number(value) : value;
	}
	return settings;
}

/** Load the optional driver, or say why it is unavailable. */
async function loadDriver(): Promise<PgModule> {
	try {
		return (await import("pg")) as unknown as PgModule;
	} catch (error) {
		throw new Error(
			`the shared engine needs the optional 'pg' driver (npm install in extension/): ${String(error)}`,
		);
	}
}

/** A quoted identifier. Queue names are derived by the bus, never taken from a record. */
function quote(identifier: string): string {
	return `"${identifier.replace(/"/g, '""')}"`;
}

/** The bus over a PostgreSQL database with the pgmq schema installed. */
export class PgBus implements Bus {
	private readonly client: PgClient;
	private readonly queues = new Map<string, string>();
	private closed = false;

	private constructor(client: PgClient, queues: Map<string, string>) {
		this.client = client;
		this.queues = queues;
	}

	/** Connect, and resolve the queue name of every address this session will touch. */
	static async open(
		location: string,
		addresses: readonly string[],
		env: Record<string, string | undefined> = process.env,
	): Promise<PgBus> {
		const module = await loadDriver();
		const client = new module.Client(pgSettings(location, env));
		await (client as unknown as { connect(): Promise<void> }).connect();
		const queues = new Map<string, string>();
		for (const address of addresses) {
			const result = await client.query("SELECT msgq.queue_name(msgq.canonical_address($1))", [
				address,
			]);
			queues.set(address, String(result.rows[0]?.queue_name ?? ""));
		}
		return new PgBus(client, queues);
	}

	/**
	 * The local engine writes synchronously, so presence can decide at once whether it is
	 * registered. A server cannot be asked synchronously, and claiming registration before the
	 * write settles would be a false claim — the roster and the session would disagree. Publishing
	 * on the shared engine needs an asynchronous presence path, which this bus does not have yet.
	 */
	publish(): never {
		throw new Error(
			"the shared engine cannot publish synchronously: presence is not registered from this session",
		);
	}

	/** Nothing to fold back: the server's own autovacuum owns this. */
	truncateWal(): void {}

	async close(): Promise<void> {
		this.closed = true;
		await this.client.end();
	}

	private async queueOf(address: string): Promise<string> {
		const known = this.queues.get(address);
		if (known !== undefined) return known;
		const result = await this.client.query(
			"SELECT msgq.queue_name(msgq.canonical_address($1))",
			[address],
		);
		const name = String(result.rows[0]?.queue_name ?? "");
		this.queues.set(address, name);
		return name;
	}

	/**
	 * One record, as the reply path needs it: an offset within one address.
	 *
	 * Offsets are per stream, so the address is not optional here even though the local engine can
	 * find a record by offset alone.
	 */
	async readOneAsync(address: string, offset: number): Promise<BusEvent | undefined> {
		const queue = await this.queueOf(address);
		const result = await this.client.query(
			`SELECT msg_id, message::text AS payload,
			        EXTRACT(EPOCH FROM enqueued_at)::bigint AS created_at
			 FROM pgmq.${quote(`q_${queue}`)} WHERE msg_id = $1`,
			[offset],
		);
		const row = result.rows[0];
		if (!row) return undefined;
		return {
			offset: Number(row.msg_id),
			topic: address,
			key: null,
			payload: JSON.parse(String(row.payload)),
			createdAt: Number(row.created_at),
		};
	}

	/** readOne is asynchronous here; the interface's synchronous form is the local engine's. */
	readOne(address: string, offset: number): BusEvent | undefined {
		// A server cannot answer synchronously. Callers that need the shared engine use
		// readOneAsync; this keeps the interface honest for the engine that can.
		void address;
		void offset;
		return undefined;
	}

	subscribe(
		address: string,
		consumer: string,
		onEvent: (event: BusEvent) => void,
		onError: (error: unknown) => void,
		onGap?: (dropped: number) => void,
	): Subscription {
		let stopped = false;
		let cursor: number | undefined;

		const pump = (async () => {
			try {
				// A consumer attaching to an address that has never been written to still needs a
				// queue to read: an empty one is the honest starting point.
				await this.client.query("SELECT msgq.ensure_queue($1)", [address]);
				const queue = await this.queueOf(address);
				await this.initializeCursor(consumer, address);
				cursor = await this.storedOffset(consumer, address);
				// Report the loss once and carry on: a gap is a warning, and a consumer that stops
				// because it missed something never catches up.
				const dropped = await this.dropped(address, cursor);
				if (dropped > 0) onGap?.(dropped);
				let waking: (() => void) | undefined;
				const channel = `pgmq.q_${queue}.INSERT`;
				const notify = () => waking?.();
				this.client.on("notification", notify);
				await this.client.query(`LISTEN ${quote(channel)}`);

				while (!stopped) {
					const result = await this.client.query(
						`SELECT msg_id, message::text AS payload,
						        EXTRACT(EPOCH FROM enqueued_at)::bigint AS created_at
						 FROM pgmq.${quote(`q_${queue}`)}
						 WHERE msg_id > $1 ORDER BY msg_id LIMIT $2`,
						[cursor ?? 0, READ_BATCH],
					);
					if (result.rows.length === 0) {
						// A notification and a timeout are both a reason to look again.
						await new Promise<void>((resolve) => {
							const timer = setTimeout(done, IDLE_WAIT_MS);
							function done() {
								clearTimeout(timer);
								waking = undefined;
								resolve();
							}
							waking = done;
						});
						continue;
					}
					for (const row of result.rows) {
						if (stopped) break;
						const offset = Number(row.msg_id);
						cursor = offset;
						onEvent({
							offset,
							topic: address,
							key: null,
							payload: JSON.parse(String(row.payload)),
							createdAt: Number(row.created_at),
						});
					}
					// The caller advances the cursor through save(); reading never does.
					await this.persist(consumer, address, cursor ?? 0);
				}
				this.client.off("notification", notify);
			} catch (error) {
				if (!stopped) onError(error);
			}
		})();

		return {
			address,
			save: (offset: number) => {
				if (cursor !== undefined && offset > cursor) cursor = offset;
			},
			close: () => {
				stopped = true;
				void pump;
			},
		};
	}

	/** Create the consumer's row if it is missing, the way the CLI does. */
	private async initializeCursor(consumer: string, address: string): Promise<void> {
		await this.client.query(
			`INSERT INTO msgq.cursor (consumer, address, last_msg_id) VALUES ($1, $2, 0)
			 ON CONFLICT (consumer, address) DO NOTHING`,
			[consumer, address],
		);
	}

	private async storedOffset(consumer: string, address: string): Promise<number> {
		const result = await this.client.query(
			"SELECT last_msg_id FROM msgq.cursor WHERE consumer = $1 AND address = $2",
			[consumer, address],
		);
		return Number(result.rows[0]?.last_msg_id ?? 0);
	}

	private async persist(consumer: string, address: string, offset: number): Promise<void> {
		await this.client.query(
			`INSERT INTO msgq.cursor (consumer, address, last_msg_id) VALUES ($1, $2, $3)
			 ON CONFLICT (consumer, address) DO UPDATE
			 SET last_msg_id = GREATEST(msgq.cursor.last_msg_id, EXCLUDED.last_msg_id)`,
			[consumer, address, offset],
		);
	}

	/** What a consumer has lost to the retention window, for the gap report. */
	async dropped(address: string, cursor: number): Promise<number> {
		const result = await this.client.query(
			`SELECT
			   (SELECT COALESCE(MAX(removed_total), 0) FROM msgq.retention WHERE topic = $1)
			 - (SELECT COALESCE(MAX(removed_total), 0) FROM msgq.retention
			    WHERE topic = $1 AND cut_offset <= $2) AS dropped`,
			[address, cursor],
		);
		return Math.max(0, Number(result.rows[0]?.dropped ?? 0));
	}
}
