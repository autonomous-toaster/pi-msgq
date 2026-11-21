// The bus over the shared engine: pgmq on PostgreSQL.
//
// Same interface as the local bus, so delivery does not learn which engine it is talking to. The
// differences are all below this line: a queue per address, a cursor in `msgq.cursor`, and a wake
// from the engine's insert notification with polling underneath.
//
// `pg` is an optional dependency: a session on a local file never loads it, and a session pointed
// at a server that cannot load it reports that rather than pretending to be connected.

import { readFileSync } from "node:fs";

import type {
	AttachmentChange,
	Bus,
	BusEvent,
	Subscription,
} from "./consumer.ts";
import { createPump } from "./consumer.ts";

/** Records fetched per read. */
/** The lease a session renews for, and half of it is when the next renewal is due. */
export const DEFAULT_LEASE = "1 hour";

const READ_BATCH = 100;

/** One page of the waiting-record count, and how far the count is allowed to page. */
const COUNT_PAGE = 1000;
const COUNT_LIMIT = 20_000;

/** Fallback wait when nothing notifies. */
// A watchdog: the LISTEN channel wakes the pump, and this only covers a missed notification.
const IDLE_WAIT_MS = 30_000;

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
 * the `MSGQ_PG*` variables supply what it does not.
 */
export function pgSettings(
	location: string,
	env: Record<string, string | undefined>,
): Record<string, unknown> {
	const settings: Record<string, unknown> = {};
	if (location.includes("://")) settings.connectionString = location;
	const map: Array<[string, string]> = [
		["MSGQ_PGHOST", "host"],
		["MSGQ_PGPORT", "port"],
		["MSGQ_PGUSER", "user"],
		["MSGQ_PGPASSWORD", "password"],
		["MSGQ_PGDATABASE", "database"],
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

/** The credential this session writes with: the environment, then the identity file's directory. */
function tokenFrom(env: Record<string, string | undefined>): string | undefined {
	const direct = env.MSGQ_TOKEN?.trim();
	if (direct) return direct;
	try {
		const home = env.HOME;
		if (!home) return undefined;
		const token = readFileSync(`${home}/.config/msgq/token`, "utf8").trim();
		return token || undefined;
	} catch {
		return undefined;
	}
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
	private readonly token: string | undefined;
	private readonly queues = new Map<string, string>();
	private closed = false;

	private constructor(client: PgClient, queues: Map<string, string>, token?: string) {
		this.client = client;
		this.queues = queues;
		this.token = token;
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
		return new PgBus(client, queues, tokenFrom(env));
	}

	/**
	 * Write through the authorizing function, so the server stamps the sender and checks the
	 * address against this identity's scopes. A refusal rejects, which the caller reports.
	 */
	async publish(address: string, record: unknown): Promise<void> {
		if (!this.token) {
			throw new Error(
				"the shared engine needs a credential to write; set MSGQ_TOKEN or write ~/.config/msgq/token",
			);
		}
		await this.client.query("SELECT msgq.publish_as($1, $2, $3::text::jsonb)", [
			this.token,
			address,
			JSON.stringify(record),
		]);
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
		const result = await this.client.query(
			"SELECT msg_id, record::text AS payload, created_at FROM msgq.read_since($1, $2, $3, 1)",
			[this.token, address, Math.max(offset - 1, 0)],
		);
		const row = result.rows[0];
		// One record after the offset before it: anything else is not the record that was asked for.
		if (!row || Number(row.msg_id) !== offset) return undefined;
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
		onState: (change: AttachmentChange) => void,
		onGap?: (dropped: number) => void,
	): Subscription {
		// Where this consumer had reached. Loaded on attach, which is why the pump is told nothing
		// about it: a server answers when asked, and the answer is this session's resume point.
		let cursor: number | undefined;
		let attached = false;
		let waking: (() => void) | undefined;
		let notify: (() => void) | undefined;

		/** Everything that has to succeed before the first read: the queue, the cursor, the wake. */
		const attach = async (): Promise<void> => {
			if (notify) this.client.off("notification", notify);
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
			const channel = `pgmq.q_${queue}.INSERT`;
			const wake = () => waking?.();
			notify = wake;
			this.client.on("notification", wake);
			await this.client.query(`LISTEN ${quote(channel)}`);
		};

		const pump = createPump({
			from: 0,
			read: async () => {
				if (!attached) {
					await attach();
					attached = true;
				}
				const result = await this.client.query(
					"SELECT msg_id, record::text AS payload, created_at FROM msgq.read_since($1, $2, $3, $4)",
					[this.token, address, cursor ?? 0, READ_BATCH],
				);
				return result.rows.map((row) => ({
					offset: Number(row.msg_id),
					topic: address,
					key: null,
					payload: JSON.parse(String(row.payload)),
					createdAt: Number(row.created_at),
				}));
			},
			wait: () =>
				new Promise<void>((resolve) => {
					// A notification and a timeout are both a reason to look again.
					const timer = setTimeout(done, IDLE_WAIT_MS);
					function done() {
						clearTimeout(timer);
						waking = undefined;
						resolve();
					}
					waking = done;
				}),
			onEvent: (event) => {
				cursor = event.offset;
				onEvent(event);
			},
			onState,
			// The resume point is the server's answer, not the argument: this engine discovers it on
			// attach, so the count is taken from where the cursor actually is.
			onWaiting: () => this.countSince(address, cursor ?? 0),
			// The caller advances the cursor through save(); reading never does. A batch is the
			// boundary at which this engine writes it back.
			onBatch: async (next) => {
				cursor = next;
				await this.persist(consumer, address, next);
			},
			onClose: () => {
				if (notify) this.client.off("notification", notify);
			},
		});

		return {
			address,
			save: (offset: number) => {
				if (cursor !== undefined && offset > cursor) cursor = offset;
			},
			attachNow: () => pump.attachNow(),
			state: () => {
				const state = pump.state();
				return state === "closed" ? "retrying" : state;
			},
			close: () => pump.close(),
		};
	}

	/**
	 * How many records follow `offset`.
	 *
	 * Counted in pages because the read surface is paged, and bounded because this is a hint for an
	 * announcement, not an inventory: a caller that needs every record reads them. A count that comes
	 * back short under-reports, which is the safe direction — the records are still all delivered.
	 */
	async countSince(address: string, offset: number): Promise<number> {
		let total = 0;
		let from = Math.max(0, offset);
		for (;;) {
			const result = await this.client.query(
				"SELECT msg_id FROM msgq.read_since($1, $2, $3, $4)",
				[this.token, address, from, COUNT_PAGE],
			);
			const rows = result.rows;
			total += rows.length;
			if (rows.length < COUNT_PAGE || total >= COUNT_LIMIT) return total;
			from = Number(rows[rows.length - 1]?.msg_id ?? from);
		}
	}

	/** A consumer with no stored position reads from the start; the function stamps the owner. */
	private async initializeCursor(consumer: string, address: string): Promise<void> {
		await this.cursor(address, consumer, 0);
	}

	private async storedOffset(consumer: string, address: string): Promise<number> {
		return this.cursor(address, consumer);
	}

	/** One function for read and write: a position belongs to the principal that wrote it. */
	private async cursor(address: string, consumer: string, offset?: number): Promise<number> {
		const result = await this.client.query(
			"SELECT msgq.cursor($1, $2, $3, $4) AS position",
			[this.token, address, consumer, offset ?? null],
		);
		return Number(result.rows[0]?.position ?? 0);
	}

	private async persist(consumer: string, address: string, offset: number): Promise<void> {
		await this.cursor(address, consumer, offset);
	}

	/**
	 * Extend the lease on this session's credential. The digest is unchanged, so sibling sessions
	 * sharing it stay logged in; a session that stayed silent past its lease recovers here.
	 */
	async renew(lease = DEFAULT_LEASE): Promise<string | undefined> {
		const result = await this.client.query(
			"SELECT msgq.renew_credential($1, $2::text::interval) AS expiry",
			[this.token, lease],
		);
		const expiry = result.rows[0]?.expiry;
		return expiry === undefined || expiry === null ? undefined : String(expiry);
	}

	/** What a consumer has lost to the retention window, for the gap report. */
	async dropped(address: string, cursor: number): Promise<number> {
		const result = await this.client.query("SELECT msgq.dropped($1, $2, $3) AS dropped", [
			this.token,
			address,
			cursor,
		]);
		return Math.max(0, Number(result.rows[0]?.dropped ?? 0));
	}
}
