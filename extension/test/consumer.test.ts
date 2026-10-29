import assert from "node:assert/strict";
import { mkdtempSync, statSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";

import honker from "@russellthehippo/honker-node";

import { Bus } from "../src/consumer.ts";

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
	publish(db, "group/a", "old-1");
	publish(db, "group/a", "old-2");

	const bus = new Bus(db, INTERVAL);
	const seen: string[] = [];
	const sub = bus.subscribe(
		"group/a",
		"c1",
		(event) => seen.push((event.payload as any).message),
		(error) => {
			throw error;
		},
	);

	await wait(300);
	assert.deepEqual(seen, [], "retained history must not be delivered");

	publish(db, "group/a", "new-1");
	await wait(400);
	assert.deepEqual(seen, ["new-1"]);

	sub.close();
	bus.close();
});

test("the cursor advances only when the caller saves", async () => {
	const db = tempDb();
	const reader = native.open(db, { watcherPollIntervalMs: INTERVAL });
	const bus = new Bus(db, INTERVAL);

	let delivered = -1;
	let cursorAtDelivery = -1;
	const sub = bus.subscribe(
		"group/a",
		"c1",
		(event) => {
			delivered = event.offset;
			cursorAtDelivery = reader.stream("group/a").getOffset("c1");
		},
		(error) => {
			throw error;
		},
	);

	await wait(200);
	const published = publish(db, "group/a", "one");
	await wait(400);

	assert.equal(delivered, published, "the record is delivered");
	assert.notEqual(cursorAtDelivery, published, "reading must not advance the cursor");

	sub.save(delivered);
	assert.equal(
		reader.stream("group/a").getOffset("c1"),
		published,
		"saving persists progress",
	);

	sub.close();
	bus.close();
	reader.close();
});

test("two consumers advance independently", async () => {
	const db = tempDb();
	const bus = new Bus(db, INTERVAL);
	const a: string[] = [];
	const b: string[] = [];

	const subA = bus.subscribe(
		"group/a",
		"a",
		(event) => a.push((event.payload as any).message),
		(error) => {
			throw error;
		},
	);
	const subB = bus.subscribe(
		"group/a",
		"b",
		(event) => b.push((event.payload as any).message),
		(error) => {
			throw error;
		},
	);

	await wait(200);
	publish(db, "group/a", "m1");
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
	const first = publish(db, "group/a", "the question");
	publish(db, "group/b", "unrelated");
	const later = publish(db, "group/a", "later");
	assert.notEqual(later, first + 1, "the offsets in this fixture must be sparse");

	const bus = new Bus(db, INTERVAL);
	const sub = bus.subscribe(
		"group/a",
		"c1",
		() => {},
		(error) => {
			throw error;
		},
	);
	const reader = native.open(db, { watcherPollIntervalMs: INTERVAL });
	const before = reader.stream("group/a").getOffset("c1");

	const found = bus.readOne("group/a", first);
	assert.equal((found?.payload as any).message, "the question");

	// An offset that belongs to another stream, or to no record at all, is not this
	// stream's record, and must not be mistaken for the one that was asked for.
	assert.equal(bus.readOne("group/a", first + 1), undefined);
	assert.equal(bus.readOne("group/b", first), undefined);
	assert.equal(
		reader.stream("group/a").getOffset("c1"),
		before,
		"reading must not advance a cursor",
	);

	reader.close();
	sub.close();
	bus.close();
});

test("resumes from the saved cursor after reopening", async () => {
	const db = tempDb();

	const bus1 = new Bus(db, INTERVAL);
	const first: string[] = [];
	let lastOffset = 0;
	const sub1 = bus1.subscribe(
		"group/a",
		"c1",
		(event) => {
			first.push((event.payload as any).message);
			lastOffset = event.offset;
		},
		(error) => {
			throw error;
		},
	);
	await wait(200);
	publish(db, "group/a", "while-running");
	await wait(400);
	assert.deepEqual(first, ["while-running"]);
	sub1.save(lastOffset);
	sub1.close();
	bus1.close();

	publish(db, "group/a", "while-away");

	const bus2 = new Bus(db, INTERVAL);
	const second: string[] = [];
	const sub2 = bus2.subscribe(
		"group/a",
		"c1",
		(event) => second.push((event.payload as any).message),
		(error) => {
			throw error;
		},
	);
	await wait(400);
	assert.deepEqual(second, ["while-away"], "a saved cursor resumes and delivers the gap");
	sub2.close();
	bus2.close();
});

test("an unsaved record is redelivered after reopening", async () => {
	const db = tempDb();

	const bus1 = new Bus(db, INTERVAL);
	const seen: string[] = [];
	const sub1 = bus1.subscribe(
		"group/a",
		"c1",
		(event) => seen.push((event.payload as any).message),
		(error) => {
			throw error;
		},
	);
	await wait(200);
	publish(db, "group/a", "not-acted-on");
	await wait(400);
	assert.deepEqual(seen, ["not-acted-on"]);
	// Deliberately no save: the record was never acted on.
	sub1.close();
	bus1.close();

	const bus2 = new Bus(db, INTERVAL);
	const again: string[] = [];
	const sub2 = bus2.subscribe(
		"group/a",
		"c1",
		(event) => again.push((event.payload as any).message),
		(error) => {
			throw error;
		},
	);
	await wait(400);
	assert.deepEqual(again, ["not-acted-on"], "an unacted record is redelivered, not lost");
	sub2.close();
	bus2.close();
});

test("close is idempotent", () => {
	const db = tempDb();
	const bus = new Bus(db, INTERVAL);
	const sub = bus.subscribe(
		"group/a",
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
	for (let i = 0; i < 200; i += 1) writer.stream("group/a").publish({ v: 1, i, payload });
	const before = walSize(db);
	assert.ok(before > 1_000_000, `expected a grown log, got ${before}`);

	const bus = new Bus(db, INTERVAL);
	bus.truncateWal();

	assert.ok(walSize(db) < before, `expected the log to shrink from ${before}`);

	// The records are in the database now, so the handle that stayed open can still read.
	assert.equal(writer.stream("group/a").readSince(0, 1).length, 1);

	bus.close();
	writer.close();
});

test("emptying the log after close does nothing", () => {
	const bus = new Bus(tempDb(), INTERVAL);
	bus.close();
	bus.truncateWal();
});
