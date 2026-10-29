// Bus consumer: a thin wrapper over the honker binding's durable streams.
//
// The binding owns the on-disk layout, the cross-process watcher, and the
// monotonic cursor. This module owns the pump loop, so the extension — not the
// binding — decides when the cursor advances past a record.

import honker from "@russellthehippo/honker-node";

/** Idle watcher cadence. Raised from the 1 ms default to keep a session quiet. */
export const WATCHER_INTERVAL_MS = 500;

/** Records fetched per read. */
const READ_BATCH = 100;

/** Fallback wait when the watcher is quiet. */
const IDLE_WAIT_MS = 1000;

/** One record read from a stream. `payload` is already parsed by the binding. */
export interface BusEvent {
	offset: number;
	topic: string;
	key: string | null;
	payload: unknown;
	createdAt: number | null;
}

/** A live subscription. */
export interface Subscription {
	address: string;
	/** Persist progress. Monotonic: a lower offset is ignored. */
	save(offset: number): void;
	close(): void;
}

interface NativeUpdateEvents {
	next(): Promise<void>;
	close(): void;
}

interface NativeStream {
	publish(payload: unknown): number;
	readSince(offset: number, limit: number): BusEvent[];
	saveOffset(consumer: string, offset: number): boolean;
	getOffset(consumer: string): number;
}

interface NativeDatabase {
	stream(name: string): NativeStream;
	updateEvents(): NativeUpdateEvents;
	query(sql: string, params?: unknown[]): Array<Record<string, unknown>>;
	close(): void;
}

interface NativeModule {
	open(path: string, options?: { watcherPollIntervalMs?: number | null }): NativeDatabase;
}

const native = honker as unknown as NativeModule;

/**
 * Race the watcher against a fallback timeout, so a quiet bus still yields and a
 * closed watcher cannot leave the pump waiting forever.
 */
function waitForUpdate(updates: NativeUpdateEvents, timeoutMs: number): Promise<void> {
	return new Promise<void>((resolve) => {
		let done = false;
		const finish = () => {
			if (done) return;
			done = true;
			clearTimeout(handle);
			resolve();
		};
		const handle = setTimeout(finish, timeoutMs);
		updates.next().then(finish, finish);
	});
}

/** An open message bus. */
export class Bus {
	private readonly db: NativeDatabase;
	private closed = false;

	constructor(dbPath: string, watcherIntervalMs: number = WATCHER_INTERVAL_MS) {
		this.db = native.open(dbPath, { watcherPollIntervalMs: watcherIntervalMs });
	}

	/**
	 * Subscribe to one address as a named consumer.
	 *
	 * The cursor is NOT advanced by reading. The caller advances it by calling
	 * `save()` once it has taken responsibility for a record, so an interruption
	 * redelivers work that was never acted on.
	 */
	subscribe(
		address: string,
		consumer: string,
		onEvent: (event: BusEvent) => void,
		onError: (error: unknown) => void,
	): Subscription {
		this.initializeCursor(consumer, address);

		const stream = this.db.stream(address);
		const updates = this.db.updateEvents();
		let cursor = stream.getOffset(consumer);
		let stopped = false;

		const pump = (async () => {
			try {
				while (!stopped) {
					const events = stream.readSince(cursor, READ_BATCH);
					if (events.length === 0) {
						await waitForUpdate(updates, IDLE_WAIT_MS);
						continue;
					}
					for (const event of events) {
						if (stopped) break;
						cursor = event.offset;
						onEvent(event);
					}
				}
			} catch (error) {
				if (!stopped) onError(error);
			} finally {
				try {
					updates.close();
				} catch {
					// Closing an already-closed subscription is not an error.
				}
			}
		})();

		return {
			address,
			save(offset) {
				if (stopped) return;
				try {
					stream.saveOffset(consumer, offset);
				} catch {
					// Progress persistence is best-effort; the record is redelivered.
				}
			},
			close() {
				if (stopped) return;
				stopped = true;
				try {
					updates.close();
				} catch {
					// Idempotent.
				}
				void pump;
			},
		};
	}

	/** Publish one record to an address. Used only for acknowledgements. */
	publish(address: string, record: unknown): number {
		return this.db.stream(address).publish(record);
	}

	/**
	 * Read one record at a known offset, without subscribing.
	 *
	 * Offsets are global to the database, so a single stream's offsets are sparse: asking
	 * for the records after `offset - 1` can return a later one. What comes back is
	 * therefore compared against the offset asked for, and a mismatch means the record is
	 * not in that stream.
	 *
	 * This advances no cursor and takes responsibility for nothing: it is how a reference
	 * is resolved to the record it names, and resolving one must not acknowledge or
	 * redeliver anything.
	 */
	readOne(address: string, offset: number): BusEvent | undefined {
		if (this.closed || offset < 0) return undefined;
		try {
			const event = this.db.stream(address).readSince(Math.max(0, offset - 1), 1)[0];
			return event && event.offset === offset ? event : undefined;
		} catch {
			return undefined;
		}
	}

	/**
	 * Fold the write-ahead log back into the database and empty it.
	 *
	 * The engine parks its automatic checkpoint at 10000 pages (~40 MB) and sets no
	 * journal size limit, so a WAL grows to that mark and then keeps it: the frames are
	 * folded in, the file is not shrunk. Truncating on the way out returns the space
	 * instead of leaving every session's peak on disk until the last handle closes.
	 *
	 * Best-effort in both directions. A checkpoint that cannot run changes nothing about
	 * the data — the frames are replayed either way — and one that finds a reader still
	 * busy gives up rather than stalling the caller.
	 */
	truncateWal(): void {
		if (this.closed) return;
		try {
			this.db.query("PRAGMA wal_checkpoint(TRUNCATE)");
		} catch {
			// Nothing is lost: the log is the database's problem, not the caller's.
		}
	}

	/** Release the engine handle. Idempotent. */
	close(): void {
		if (this.closed) return;
		this.closed = true;
		try {
			this.db.close();
		} catch {
			// Closing an already-closed handle is not an error.
		}
	}

	/**
	 * Start a consumer that has never been seen at the current end of its
	 * stream, so first attach delivers only new records. The binding's default
	 * is offset 0, which would replay a topic's entire retained history.
	 *
	 * The binding exposes no head-offset API, so this is the one place the
	 * extension reads an engine table directly. It is the same query the CLI's
	 * `topics` command uses.
	 */
	private initializeCursor(consumer: string, address: string): void {
		if (this.hasCursor(consumer, address)) return;
		this.db.stream(address).saveOffset(consumer, this.headOffset(address));
	}

	private hasCursor(consumer: string, address: string): boolean {
		const rows = this.db.query(
			"SELECT 1 AS present FROM _honker_stream_consumers WHERE name = ? AND topic = ?",
			[consumer, address],
		);
		return rows.length > 0;
	}

	private headOffset(address: string): number {
		const rows = this.db.query(
			"SELECT COALESCE(MAX(offset), 0) AS head FROM _honker_stream WHERE topic = ?",
			[address],
		);
		return Number(rows[0]?.head ?? 0);
	}
}

/** Open a second handle for tests and tooling. */
export function openRaw(dbPath: string): NativeDatabase {
	return native.open(dbPath, { watcherPollIntervalMs: WATCHER_INTERVAL_MS });
}
