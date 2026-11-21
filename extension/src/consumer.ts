// Bus consumer: a thin wrapper over the honker binding's durable streams.
//
// The binding owns the on-disk layout, the cross-process watcher, and the
// monotonic cursor. This module owns the pump loop, so the extension — not the
// binding — decides when the cursor advances past a record.
//
// The pump is the one place that knows what survives a failure: the cursor, the
// intent to consume, and the address. An engine that can read synchronously and
// one that must await both drive the same loop, so neither can invent its own
// rule about what a broken read costs.

import honker from "@russellthehippo/honker-node";

/** Idle watcher cadence. Raised from the 1 ms default to keep a session quiet. */
export const WATCHER_INTERVAL_MS = 500;

/** Records fetched per read. */
const READ_BATCH = 100;

/** Fallback wait when the watcher is quiet. */
const IDLE_WAIT_MS = 1000;

/** First delay after a failed read. */
export const RETRY_INITIAL_MS = 100;

/** Longest delay between attempts: the pump never gives up, so this caps what a dead bus costs. */
export const RETRY_MAX_MS = 5_000;

/** How much of each delay is jitter, so a bus that breaks once does not wake every session at once. */
const RETRY_JITTER = 0.2;

/** Consecutive failures after which a still-detached subscription says so once more. */
export const RETRY_REPORT_AFTER = 20;

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
 *
 * A watcher that cannot push at all leaves the timeout to do the work: falling back to polling
 * costs a read a second, which is what the alternative — spinning on a rejected promise — would
 * cost anyway, only without the pause.
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
		updates.next().then(finish, () => {
			// Push unavailable: the timeout is now the only reason this read happens again.
		});
	});
}

/** One record read from a stream. `payload` is already parsed by the binding. */
export interface BusEvent {
	offset: number;
	topic: string;
	key: string | null;
	payload: unknown;
	createdAt: number | null;
}

/** What a subscription is doing. `attaching` is the state before the first read answers. */
export type AttachmentState = "attaching" | "attached" | "retrying";

/** One report from a subscription: a change of state, or a threshold notice. */
export interface AttachmentChange {
	state: Exclude<AttachmentState, "attaching">;
	/** Why the pump is retrying. Absent when it is attached. */
	reason?: string;
	/** Consecutive failures at the time of the report. */
	failures: number;
	/** A threshold notice about the same failure rather than a change of state. */
	repeat?: boolean;
	/** On attach: how many records were waiting at the offset the pump resumed from. */
	waiting?: number;
}

/** A live subscription. */
export interface Subscription {
	address: string;
	/** Persist progress. Monotonic: a lower offset is ignored. */
	save(offset: number): void;
	/** Try now instead of waiting out the remaining backoff. */
	attachNow(): void;
	/** What the subscription is doing. */
	state(): AttachmentState;
	close(): void;
}

/** What the pump needs from an engine. Every field is a function, so a test can drive it. */
export interface PumpOptions {
	/** Where to resume. A fresh attach reads from the persisted offset. */
	from: number;
	/** Read the next batch after `from`. Throwing is how an engine says it is unwell. */
	read(from: number): BusEvent[] | Promise<BusEvent[]>;
	/** Wait for something to happen, or for the idle timeout to expire. */
	wait(): Promise<void>;
	onEvent(event: BusEvent): void;
	onState?(change: AttachmentChange): void;
	/** How many records are waiting at `from`. A count that fails costs the count, not the attach. */
	onWaiting?(from: number): number | Promise<number>;
	/** Called with the cursor after a batch, for an engine that persists progress itself. */
	onBatch?(cursor: number): void | Promise<void>;
	/** Called once when the pump stops. */
	onClose?(): void;
	/** Delay before retry `failures`. Injectable so a test needs no clock. */
	delay?(failures: number): number;
	sleep?(ms: number): Promise<void>;
}

/** The loop behind one subscription. */
export interface Pump {
	attachNow(): void;
	close(): void;
	state(): AttachmentState | "closed";
	cursor(): number;
}

/** The delay before retry `failures`, capped and jittered. */
export function retryDelay(
	failures: number,
	random: () => number = Math.random,
): number {
	const base = Math.min(RETRY_MAX_MS, RETRY_INITIAL_MS * 2 ** Math.max(0, failures - 1));
	const spread = base * RETRY_JITTER;
	return Math.round(base - spread + random() * spread * 2);
}

function messageOf(error: unknown): string {
	if (error instanceof Error) return error.message || error.name;
	return String(error);
}

function defaultSleep(ms: number): Promise<void> {
	return new Promise<void>((resolve) => {
		const handle = setTimeout(resolve, ms);
		// A retry timer must not be the reason a process stays alive.
		handle.unref?.();
	});
}

/**
 * Run one subscription's read loop.
 *
 * Reading is a loop, not an event: a failed read backs off and tries again from the offset it
 * already had, and only a successful read advances the cursor. Nothing here is fatal — the pump
 * stops when it is closed, and a bus that stays broken costs one timer per subscription.
 */
export function createPump(options: PumpOptions): Pump {
	const delay = options.delay ?? retryDelay;
	const sleep = options.sleep ?? defaultSleep;

	let cursor = options.from;
	let failures = 0;
	let state: AttachmentState | "closed" = "attaching";
	let nudge: (() => void) | undefined;
	let stopped = false;

	/** Wait out the backoff, unless a caller asks for an attempt sooner. */
	function backoff(): Promise<void> {
		const ms = delay(failures);
		return new Promise<void>((resolve) => {
			let settled = false;
			const finish = () => {
				if (settled) return;
				settled = true;
				nudge = undefined;
				resolve();
			};
			nudge = finish;
			void sleep(ms).then(finish, finish);
		});
	}

	/** The number waiting, or 0 when the engine cannot say. A count never blocks an attach. */
	async function waitingAt(from: number): Promise<number> {
		if (!options.onWaiting) return 0;
		try {
			const count = await options.onWaiting(from);
			return Number.isFinite(count) && count > 0 ? Math.trunc(count) : 0;
		} catch {
			return 0;
		}
	}

	const loop = (async () => {
		while (!stopped) {
			try {
				const events = await options.read(cursor);
				if (stopped) break;
				failures = 0;
				if (state !== "attached") {
					const waiting = await waitingAt(cursor);
					if (stopped) break;
					state = "attached";
					options.onState?.({ state, failures: 0, waiting });
				}
				for (const event of events) {
					if (stopped) break;
					// The cursor moves with the handover: after this line the record is this
					// session's responsibility, and a failure below is a delivery failure, not a
					// read failure. The engine's stored offset still only moves on save().
					cursor = event.offset;
					options.onEvent(event);
				}
				if (events.length > 0) {
					await options.onBatch?.(cursor);
					continue;
				}
				await options.wait();
			} catch (error) {
				if (stopped) break;
				failures += 1;
				const reason = messageOf(error);
				if (state !== "retrying") {
					state = "retrying";
					options.onState?.({ state, reason, failures });
				} else if (failures === RETRY_REPORT_AFTER) {
					options.onState?.({ state, reason, failures, repeat: true });
				}
				await backoff();
			}
		}
	})();

	return {
		attachNow() {
			// A nudge only shortens a wait that is already happening; a pump that is reading will
			// read again on its own, so nudging twice attaches once.
			nudge?.();
		},
		close() {
			if (stopped) return;
			stopped = true;
			nudge?.();
			options.onClose?.();
			void loop;
		},
		state() {
			return stopped ? "closed" : state;
		},
		cursor() {
			return cursor;
		},
	};
}

/**
 * An open message bus.
 *
 * The interface is what the session depends on, so a second engine can be added without touching
 * delivery: only this module knows about tables, cursors and the native binding.
 */
export interface Bus {
	subscribe(
		address: string,
		consumer: string,
		onEvent: (event: BusEvent) => void,
		/** State changes only: a retrying subscription then an attached one, never per attempt. */
		onState: (change: AttachmentChange) => void,
		/** Called once with how many records the retention window removed before this consumer's
		 * cursor, when the engine can tell. A gap is reported, never fatal. */
		onGap?: (dropped: number) => void,
	): Subscription;
	/** How many records are retained after `offset`. A read: it moves no cursor. */
	countSince(address: string, offset: number): number | Promise<number>;
	readOne(address: string, offset: number): BusEvent | undefined;
	/** Write a record. A file answers at once; a server answers when the write has landed, so a
	 * caller that must know whether it landed awaits the result. */
	publish(address: string, record: unknown): number | void | Promise<void>;
	/** Fold the write-ahead log back, where the engine has one. */
	truncateWal(): void;
	close(): void | Promise<void>;
}

/** The bus over the local engine: one SQLite file, read through the native binding. */
export class HonkerBus implements Bus {
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
		onState: (change: AttachmentChange) => void,
		_onGap?: (dropped: number) => void,
	): Subscription {
		this.initializeCursor(consumer, address);

		const stream = this.db.stream(address);
		const updates = this.db.updateEvents();
		const pump = createPump({
			from: stream.getOffset(consumer),
			read: (from) => stream.readSince(from, READ_BATCH),
			wait: () => waitForUpdate(updates, IDLE_WAIT_MS),
			onWaiting: (from) => this.countSince(address, from),
			onEvent,
			onState,
			onClose: () => {
				try {
					updates.close();
				} catch {
					// Closing an already-closed watcher is not an error.
				}
			},
		});

		return {
			address,
			save(offset) {
				if (pump.state() === "closed") return;
				try {
					stream.saveOffset(consumer, offset);
				} catch {
					// Progress persistence is best-effort; the record is redelivered.
				}
			},
			attachNow() {
				pump.attachNow();
			},
			state() {
				const state = pump.state();
				return state === "closed" ? "retrying" : state;
			},
			close() {
				pump.close();
			},
		};
	}

	/**
	 * How many records are retained on an address after `offset`.
	 *
	 * A read like any other: it moves no cursor and changes no counter. Needed because the
	 * announcement of a replayed backlog has to precede the records, and a batch would truncate the
	 * count at its own size.
	 */
	countSince(address: string, offset: number): number {
		if (this.closed) return 0;
		try {
			const rows = this.db.query(
				"SELECT COUNT(*) AS waiting FROM _honker_stream WHERE topic = ? AND offset > ?",
				[address, Math.max(0, offset)],
			);
			return Math.max(0, Number(rows[0]?.waiting ?? 0));
		} catch {
			return 0;
		}
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
	 * The binding exposes no head-offset API, so this is the second place this
	 * module reads an engine table directly (the first is `countSince`). Both are
	 * reads of the stream's own offsets, which no cursor or counter depends on.
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
