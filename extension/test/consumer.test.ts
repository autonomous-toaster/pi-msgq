import assert from "node:assert/strict";
import { mkdtempSync, statSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";
import { DatabaseSync } from "node:sqlite";

import honker from "@russellthehippo/honker-node";

import {
	HonkerBus,
	RETRY_MAX_MS,
	RETRY_REPORT_AFTER,
	createPump,
	openRaw,
	retryDelay,
} from "../src/consumer.ts";

// biome-ignore lint/suspicious/noExplicitAny: the binding is a CJS module without usable ESM types.
const native = honker as any;
const INTERVAL = 50;

function tempDb(): string {
	return join(mkdtempSync(join(tmpdir(), "msgq-ext-")), "msgq.db");
}

/** The size of a database's write-ahead log, or 0 before one exists. */
function walSize(db: string): number {
	try {
		return statSync(`${db}-wal`).size;
	} catch {
		return 0;
	}
}

function wait(ms: number): Promise<void> {
	return new Promise((resolve) => setTimeout(resolve, ms));
}

function publish(db: string, address: string, message: string): number {
	const handle = native.open(db, { watcherPollIntervalMs: INTERVAL });
	const offset = handle
		.stream(address)
		.publish({ v: 1, type: "note", from: "cli/x", to: address, message });
	handle.close();
	return offset;
}

test("first attach delivers only new records, never the retained history", async () => {
	const db = tempDb();
	publish(db, "session/a", "old-1");
	publish(db, "session/a", "old-2");

	const bus = new HonkerBus(db, INTERVAL);
	const seen: string[] = [];
	const sub = bus.subscribe(
		"session/a",
		"c1",
		(event) => seen.push((event.payload as any).message),
		() => {},
	);

	await wait(300);
	assert.deepEqual(seen, [], "retained history must not be delivered");

	publish(db, "session/a", "new-1");
	await wait(400);
	assert.deepEqual(seen, ["new-1"]);

	sub.close();
	bus.close();
});

test("the cursor advances only when the caller saves", async () => {
	const db = tempDb();
	const reader = native.open(db, { watcherPollIntervalMs: INTERVAL });
	const bus = new HonkerBus(db, INTERVAL);

	let delivered = -1;
	let cursorAtDelivery = -1;
	const sub = bus.subscribe(
		"session/a",
		"c1",
		(event) => {
			delivered = event.offset;
			cursorAtDelivery = reader.stream("session/a").getOffset("c1");
		},
		() => {},
	);

	await wait(200);
	const published = publish(db, "session/a", "one");
	await wait(400);

	assert.equal(delivered, published, "the record is delivered");
	assert.notEqual(cursorAtDelivery, published, "reading must not advance the cursor");

	sub.save(delivered);
	assert.equal(
		reader.stream("session/a").getOffset("c1"),
		published,
		"saving persists progress",
	);

	sub.close();
	bus.close();
	reader.close();
});

test("two consumers advance independently", async () => {
	const db = tempDb();
	const bus = new HonkerBus(db, INTERVAL);
	const a: string[] = [];
	const b: string[] = [];

	const subA = bus.subscribe(
		"session/a",
		"a",
		(event) => a.push((event.payload as any).message),
		() => {},
	);
	const subB = bus.subscribe(
		"session/a",
		"b",
		(event) => b.push((event.payload as any).message),
		() => {},
	);

	await wait(200);
	publish(db, "session/a", "m1");
	await wait(400);

	assert.deepEqual(a, ["m1"]);
	assert.deepEqual(b, ["m1"]);

	subA.close();
	subB.close();
	bus.close();
});

test("a single record can be read at a known offset without consuming", () => {
	const db = tempDb();
	// Offsets are global to the database, so a stream's are sparse: publishing to another
	// stream in between is what makes the offset check below mean something.
	const first = publish(db, "session/a", "the question");
	publish(db, "session/b", "unrelated");
	const later = publish(db, "session/a", "later");
	assert.notEqual(later, first + 1, "the offsets in this fixture must be sparse");

	const bus = new HonkerBus(db, INTERVAL);
	const sub = bus.subscribe(
		"session/a",
		"c1",
		() => {},
		() => {},
	);
	const reader = native.open(db, { watcherPollIntervalMs: INTERVAL });
	const before = reader.stream("session/a").getOffset("c1");

	const found = bus.readOne("session/a", first);
	assert.equal((found?.payload as any).message, "the question");

	// An offset that belongs to another stream, or to no record at all, is not this
	// stream's record, and must not be mistaken for the one that was asked for.
	assert.equal(bus.readOne("session/a", first + 1), undefined);
	assert.equal(bus.readOne("session/b", first), undefined);
	assert.equal(
		reader.stream("session/a").getOffset("c1"),
		before,
		"reading must not advance a cursor",
	);

	reader.close();
	sub.close();
	bus.close();
});

test("resumes from the saved cursor after reopening", async () => {
	const db = tempDb();

	const bus1 = new HonkerBus(db, INTERVAL);
	const first: string[] = [];
	let lastOffset = 0;
	const sub1 = bus1.subscribe(
		"session/a",
		"c1",
		(event) => {
			first.push((event.payload as any).message);
			lastOffset = event.offset;
		},
		() => {},
	);
	await wait(200);
	publish(db, "session/a", "while-running");
	await wait(400);
	assert.deepEqual(first, ["while-running"]);
	sub1.save(lastOffset);
	sub1.close();
	bus1.close();

	publish(db, "session/a", "while-away");

	const bus2 = new HonkerBus(db, INTERVAL);
	const second: string[] = [];
	const sub2 = bus2.subscribe(
		"session/a",
		"c1",
		(event) => second.push((event.payload as any).message),
		() => {},
	);
	await wait(400);
	assert.deepEqual(second, ["while-away"], "a saved cursor resumes and delivers the gap");
	sub2.close();
	bus2.close();
});

test("an unsaved record is redelivered after reopening", async () => {
	const db = tempDb();

	const bus1 = new HonkerBus(db, INTERVAL);
	const seen: string[] = [];
	const sub1 = bus1.subscribe(
		"session/a",
		"c1",
		(event) => seen.push((event.payload as any).message),
		() => {},
	);
	await wait(200);
	publish(db, "session/a", "not-acted-on");
	await wait(400);
	assert.deepEqual(seen, ["not-acted-on"]);
	// Deliberately no save: the record was never acted on.
	sub1.close();
	bus1.close();

	const bus2 = new HonkerBus(db, INTERVAL);
	const again: string[] = [];
	const sub2 = bus2.subscribe(
		"session/a",
		"c1",
		(event) => again.push((event.payload as any).message),
		() => {},
	);
	await wait(400);
	assert.deepEqual(again, ["not-acted-on"], "an unacted record is redelivered, not lost");
	sub2.close();
	bus2.close();
});

test("close is idempotent", () => {
	const db = tempDb();
	const bus = new HonkerBus(db, INTERVAL);
	const sub = bus.subscribe(
		"session/a",
		"c1",
		() => {},
		() => {},
	);
	sub.close();
	sub.close();
	bus.close();
	bus.close();
	assert.ok(true);
});

test("the log is emptied on the way out, with another handle still open", () => {
	const db = tempDb();
	const writer = native.open(db, { watcherPollIntervalMs: INTERVAL });
	const payload = "x".repeat(10_000);
	for (let i = 0; i < 200; i += 1) writer.stream("session/a").publish({ v: 1, i, payload });
	const before = walSize(db);
	assert.ok(before > 1_000_000, `expected a grown log, got ${before}`);

	const bus = new HonkerBus(db, INTERVAL);
	bus.truncateWal();

	assert.ok(walSize(db) < before, `expected the log to shrink from ${before}`);

	// The records are in the database now, so the handle that stayed open can still read.
	assert.equal(writer.stream("session/a").readSince(0, 1).length, 1);

	bus.close();
	writer.close();
});

test("emptying the log after close does nothing", () => {
	const bus = new HonkerBus(tempDb(), INTERVAL);
	bus.close();
	bus.truncateWal();
});

// ---- the pump: what a failed read costs -------------------------------------------------------

/** A record as the pump sees it: only the offset matters to the loop. */
function record(offset: number): {
	offset: number;
	topic: string;
	key: null;
	payload: unknown;
	createdAt: null;
} {
	return { offset, topic: "session/a", key: null, payload: { message: `m${offset}` }, createdAt: null };
}

/** Let the pump's microtasks run without waiting on a wall clock. */
async function tick(turns = 8): Promise<void> {
	for (let i = 0; i < turns; i += 1) await Promise.resolve();
}

test("countSince counts what follows an offset and moves no cursor", () => {
	const db = tempDb();
	const first = publish(db, "session/a", "one");
	const second = publish(db, "session/a", "two");
	publish(db, "session/b", "elsewhere");

	const bus = new HonkerBus(db, INTERVAL);
	assert.equal(bus.countSince("session/a", 0), 2);
	assert.equal(bus.countSince("session/a", first), 1);
	assert.equal(bus.countSince("session/a", second), 0);
	// Nothing attached, so there is no cursor to move; the point is that counting did not need one.
	bus.close();
});

test("a failed read is retried from the same cursor and reported once", async () => {
	const delivered: number[] = [];
	const reported: string[] = [];
	let attempt = 0;

	const pump = createPump({
		from: 10,
		read: () => {
			attempt += 1;
			if (attempt <= 2) throw new Error("database is locked");
			return attempt === 3 ? [record(11), record(12)] : [];
		},
		wait: () => new Promise<void>((resolve) => setImmediate(resolve)),
		onEvent: (event) => delivered.push(event.offset),
		onState: (change) => reported.push(change.state),
		sleep: () => Promise.resolve(),
	});

	await tick(30);
	pump.close();

	assert.deepEqual(delivered, [11, 12], "every record is delivered exactly once after the retries");
	assert.deepEqual(reported, ["retrying", "attached"], "one detachment, one recovery, whatever the retries");
	assert.equal(pump.cursor(), 12, "the resume point is the last record handed over");
});

test("the cursor does not move while reads fail", async () => {
	const pump = createPump({
		from: 40,
		read: () => {
			throw new Error("database is closed");
		},
		wait: () => Promise.resolve(),
		onEvent: () => {},
		onState: () => {},
		sleep: () => Promise.resolve(),
	});

	await tick(20);
	assert.equal(pump.cursor(), 40, "a failure before any read skips nothing and replays nothing");
	pump.close();
});

test("the backoff grows to the cap and stays there", () => {
	const flat = () => 0.5;
	assert.equal(retryDelay(1, flat), 100);
	assert.equal(retryDelay(2, flat), 200);
	assert.equal(retryDelay(5, flat), 1_600);
	assert.equal(retryDelay(20, flat), RETRY_MAX_MS);
	assert.equal(retryDelay(500, flat), RETRY_MAX_MS, "the delay never exceeds the cap");

	// Jitter stays inside its band, so a bus that breaks once does not wake every session at once.
	assert.equal(retryDelay(1, () => 0), 80);
	assert.equal(retryDelay(1, () => 1), 120);
});

test("a subscription that keeps failing reports the threshold once and keeps trying", async () => {
	const delays: number[] = [];
	const repeats: number[] = [];
	let reads = 0;

	const pump = createPump({
		from: 0,
		read: () => {
			reads += 1;
			if (reads <= 25) throw new Error("the store is unwell");
			return [];
		},
		wait: () => new Promise<void>((resolve) => setTimeout(resolve, 5)),
		onEvent: () => {},
		onState: (change) => {
			if (change.repeat) repeats.push(change.failures);
		},
		delay: (failures) => retryDelay(failures, () => 0.5),
		sleep: async (ms) => {
			delays.push(ms);
			await Promise.resolve();
		},
	});

	await tick(200);
	pump.close();

	assert.ok(delays.length >= RETRY_REPORT_AFTER, `expected at least ${RETRY_REPORT_AFTER} delays, got ${delays.length}`);
	assert.deepEqual(delays.slice(0, 7), [100, 200, 400, 800, 1_600, 3_200, 5_000]);
	assert.equal(Math.max(...delays), RETRY_MAX_MS);
	assert.deepEqual(repeats, [RETRY_REPORT_AFTER], "the same failure is repeated once, not on every attempt");
});

test("a nudge during a retry attaches once, not twice", async () => {
	const waiting: Array<() => void> = [];
	let reads = 0;

	const pump = createPump({
		from: 0,
		read: () => {
			reads += 1;
			throw new Error("database is locked");
		},
		wait: () => Promise.resolve(),
		onEvent: () => {},
		sleep: () =>
			new Promise<void>((resolve) => {
				waiting.push(resolve);
			}),
	});

	await tick();
	assert.equal(reads, 1, "the first attempt failed and is waiting out its backoff");

	pump.attachNow();
	pump.attachNow();
	await tick();

	assert.equal(reads, 2, "two nudges shorten one wait, so they buy one attempt");
	assert.equal(waiting.length, 2, "the second attempt has its own backoff, not a second pump");
	pump.close();
});

test("an engine that goes away detaches the subscription instead of ending it", async () => {
	const db = tempDb();
	const bus = new HonkerBus(db, INTERVAL);
	const states: string[] = [];
	const sub = bus.subscribe(
		"session/a",
		"c1",
		() => {},
		(change) => states.push(change.state),
	);

	await wait(200);
	publish(db, "session/a", "while-attached");
	await wait(300);
	assert.deepEqual(states, ["attached"]);

	// The store goes bad underneath the pump — the table it reads is gone — which is the failure the
	// extension used to turn into a permanent disable.
	const raw = openRaw(db);
	raw.query("DROP TABLE _honker_stream");
	raw.close();
	await wait(300);

	assert.deepEqual(states, ["attached", "retrying"], "the subscription reports the detachment");
	sub.close();
	bus.close();
});

test("an attach behind the head says how many records were waiting", async () => {
	const db = tempDb();

	const bus1 = new HonkerBus(db, INTERVAL);
	let lastOffset = 0;
	const sub1 = bus1.subscribe(
		"session/a",
		"c1",
		(event) => {
			lastOffset = event.offset;
		},
		() => {},
	);
	await wait(200);
	publish(db, "session/a", "acted-on");
	await wait(300);
	sub1.save(lastOffset);
	sub1.close();
	bus1.close();

	publish(db, "session/a", "waiting-1");
	publish(db, "session/a", "waiting-2");

	const bus2 = new HonkerBus(db, INTERVAL);
	const changes: Array<{ state: string; waiting?: number }> = [];
	const seen: string[] = [];
	const sub2 = bus2.subscribe(
		"session/a",
		"c1",
		(event) => seen.push((event.payload as any).message),
		(change) => changes.push({ state: change.state, waiting: change.waiting }),
	);
	await wait(400);

	assert.deepEqual(changes, [{ state: "attached", waiting: 2 }], "the backlog is counted before it is read");
	assert.deepEqual(seen, ["waiting-1", "waiting-2"]);
	sub2.close();
	bus2.close();
});

test("a replay after a failure neither duplicates nor drops a record", async () => {
	const delivered: number[] = [];
	// The script: two records, a failure, two more. The pump must hand over each of the four once.
	let attempt = 0;
	const pump = createPump({
		from: 0,
		read: () => {
			attempt += 1;
			if (attempt === 1) return [record(1), record(2)];
			if (attempt === 2) throw new Error("the store went away");
			if (attempt === 3) return [record(3), record(4)];
			return [];
		},
		wait: () => new Promise<void>((resolve) => setImmediate(resolve)),
		onEvent: (event) => delivered.push(event.offset),
		sleep: () => Promise.resolve(),
	});

	await tick(40);
	pump.close();

	assert.deepEqual(delivered, [1, 2, 3, 4], "the retry resumes after the last handed-over record");
	assert.equal(pump.cursor(), 4);
});

test("a store that is locked detaches the subscription, which recovers when the lock clears", async () => {
	const db = tempDb();
	// A rollback-mode journal is what makes a writer's exclusive lock block a reader; in WAL a reader
	// never waits, so there would be no failure to recover from.
	const setup = new DatabaseSync(db);
	setup.exec("PRAGMA journal_mode=DELETE");
	setup.close();

	const bus = new HonkerBus(db, INTERVAL);
	const states: string[] = [];
	const seen: string[] = [];
	const reader = openRaw(db);
	const sub = bus.subscribe(
		"session/a",
		"c1",
		(event) => {
			seen.push((event.payload as any).message);
			sub.save(event.offset);
		},
		(change) => states.push(change.state),
	);
	const locker = new DatabaseSync(db);
	try {
		await wait(300);
		publish(db, "session/a", "before-the-lock");
		await wait(400);
		assert.deepEqual(seen, ["before-the-lock"]);
		assert.deepEqual(states, ["attached"]);
		const actedOn = reader.stream("session/a").getOffset("c1");
		assert.ok(actedOn > 0, "the delivered record was saved");

		locker.exec("BEGIN EXCLUSIVE");
		await wait(800);
		assert.deepEqual(states, ["attached", "retrying"], "the lock is a detachment, not an error");

		locker.exec("COMMIT");
		await wait(700);
		assert.deepEqual(states, ["attached", "retrying", "attached"], "the subscription recovers by itself");
		assert.equal(
			reader.stream("session/a").getOffset("c1"),
			actedOn,
			"nothing was saved during the outage, and nothing was skipped by it",
		);
	} finally {
		try {
			locker.exec("COMMIT");
		} catch {
			// Already committed, or the lock never landed.
		}
		locker.close();
	}

	publish(db, "session/a", "after-the-lock");
	await wait(600);
	assert.deepEqual(seen, ["before-the-lock", "after-the-lock"]);
	assert.ok(reader.stream("session/a").getOffset("c1") > 0, "the cursor advances once delivery resumes");

	sub.close();
	bus.close();
	reader.close();
});

test("a session that died mid-turn replays its backlog as one batch", async () => {
	const db = tempDb();

	// The process that died: it acted on one record and saved that, then vanished without closing
	// anything — which is the only thing a kill -9 leaves behind.
	const dying = new HonkerBus(db, INTERVAL);
	let actedOn = 0;
	const dyingSub = dying.subscribe(
		"session/a",
		"c1",
		(event) => {
			actedOn = event.offset;
			dyingSub.save(event.offset);
		},
		() => {},
	);
	await wait(300);
	publish(db, "session/a", "acted-on");
	await wait(400);
	dyingSub.save(actedOn);
	assert.ok(actedOn > 0);

	publish(db, "session/a", "waiting-1");
	publish(db, "session/a", "waiting-2");
	publish(db, "session/a", "waiting-3");

	// The replacement process: same consumer, same address, nothing else in common.
	const bus = new HonkerBus(db, INTERVAL);
	const changes: Array<{ state: string; waiting?: number }> = [];
	const seen: string[] = [];
	const sub = bus.subscribe(
		"session/a",
		"c1",
		(event) => seen.push((event.payload as any).message),
		(change) => changes.push({ state: change.state, waiting: change.waiting }),
	);
	await wait(500);

	assert.deepEqual(changes, [{ state: "attached", waiting: 3 }], "one attach, and it says how much was waiting");
	assert.deepEqual(seen, ["waiting-1", "waiting-2", "waiting-3"], "the backlog arrives in order, once");

	sub.close();
	bus.close();
	dyingSub.close();
	dying.close();
});
