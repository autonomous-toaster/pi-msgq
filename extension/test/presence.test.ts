import assert from "node:assert/strict";
import { mkdtempSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";

import { Bus, openRaw } from "../src/consumer.ts";
import { PRESENCE_ADDRESS, Presence, PRESENCE_TTL_S } from "../src/presence.ts";
import type { MsgqRecord } from "../src/record.ts";

const INTERVAL = 50;

function tempDb(): string {
	return join(mkdtempSync(join(tmpdir(), "msgq-presence-")), "msgq.db");
}

/** Every record published to the presence address, as the bus stored it. */
function presenceRecords(db: string): MsgqRecord[] {
	const native = openRaw(db) as any;
	const rows = native.query("SELECT payload FROM _honker_stream WHERE topic = ? ORDER BY offset", [
		PRESENCE_ADDRESS,
	]);
	native.close();
	return rows.map((row: { payload: string }) => JSON.parse(row.payload) as MsgqRecord);
}

function presenceEvent(record: MsgqRecord): Record<string, unknown> {
	return (record.event ?? {}) as Record<string, unknown>;
}

function makePresence(
	db: string,
	over: Partial<{ state: any; alias: string; address: string; participant: string; ttlS: number }> = {},
) {
	const bus = new Bus(db, INTERVAL);
	const presence = new Presence(bus, {
		address: over.address ?? "session/api-worker",
		participant: over.participant ?? "01a127d9",
		alias: "alias" in over ? over.alias : "api-worker",
		state: over.state ?? "idle",
		ttlS: over.ttlS ?? 30,
	});
	return { bus, presence };
}

test("start publishes one presence record", () => {
	const db = tempDb();
	const { bus, presence } = makePresence(db);

	assert.equal(presence.start(), true);
	assert.equal(presence.held, true);

	const [record] = presenceRecords(db);
	assert.equal(record.type, "event");
	assert.equal(record.from, "session/api-worker");
	assert.equal(record.to, PRESENCE_ADDRESS);
	assert.equal(record.v, 1);
	const event = presenceEvent(record);
	assert.equal(event.kind, "presence");
	assert.equal(event.state, "idle");
	assert.equal(event.ttl, 30);
	assert.equal(event.participant, "01a127d9");
	assert.equal(event.alias, "api-worker");

	presence.stop();
	bus.close();
});

test("a participant with no alias publishes without one", () => {
	const db = tempDb();
	const { bus, presence } = makePresence(db, { alias: undefined, address: "session/01a127d9" });

	presence.start();
	const [record] = presenceRecords(db);
	assert.equal(presenceEvent(record).alias, undefined);
	assert.equal(record.from, "session/01a127d9");

	presence.stop();
	bus.close();
});

test("renew publishes a newer record rather than editing the old one", () => {
	const db = tempDb();
	const { bus, presence } = makePresence(db);
	presence.start();
	presence.renew();

	const records = presenceRecords(db);
	assert.equal(records.length, 2, "each renewal is a new record, so the newest one is the truth");

	presence.stop();
	bus.close();
});

test("a state change is published promptly", () => {
	const db = tempDb();
	const { bus, presence } = makePresence(db);
	presence.start();
	assert.equal(presence.setState("busy"), true);

	const records = presenceRecords(db);
	assert.equal(records.length, 2);
	assert.equal(presenceEvent(records[1]).state, "busy");
	// Setting the same state again publishes nothing new.
	assert.equal(presence.setState("busy"), true);
	assert.equal(presenceRecords(db).length, 2);

	presence.stop();
	bus.close();
});

test("a rename publishes under the new identity", () => {
	const db = tempDb();
	const { bus, presence } = makePresence(db);
	presence.start();

	assert.equal(presence.setOwner({ address: "session/reviewer", alias: "reviewer" }), true);
	const records = presenceRecords(db);
	assert.equal(records.length, 2);
	assert.equal(records[1].from, "session/reviewer");
	assert.equal(presenceEvent(records[1]).alias, "reviewer");

	presence.stop();
	bus.close();
});

test("stop retracts presence, and nothing is published afterwards", () => {
	const db = tempDb();
	const { bus, presence } = makePresence(db);
	presence.start();
	presence.stop();

	const records = presenceRecords(db);
	const last = records[records.length - 1];
	assert.equal(presenceEvent(last).state, "gone");
	assert.equal(presence.held, false);
	assert.equal(presence.start(), false, "a retracted presence is closed");
	assert.equal(presence.renew(), false);

	presence.stop(); // idempotent
	assert.equal(presenceRecords(db).length, records.length);

	bus.close();
});

test("the default lifetime is used when none is given", () => {
	const db = tempDb();
	const bus = new Bus(db, INTERVAL);
	const presence = new Presence(bus, {
		address: "session/x",
		participant: "01a127d9",
		state: "idle",
	});

	presence.start();
	assert.equal(presenceEvent(presenceRecords(db)[0]).ttl, PRESENCE_TTL_S);

	presence.stop();
	bus.close();
});

/** A bus whose writes fail, standing in for the engine returning an error — a full disk, most often. */
function failingBus(): Bus {
	return {
		publish() {
			throw new Error("database or disk is full");
		},
	} as unknown as Bus;
}

test("a publish that fails is not fatal and keeps its reason", () => {
	const presence = new Presence(failingBus(), {
		address: "session/x",
		participant: "01a127d9",
		state: "idle",
	});

	assert.equal(presence.start(), false);
	assert.equal(presence.held, false, "a presence that was not published is not claimed");
	assert.match(String((presence.error as Error).message), /disk is full/);
});

test("a renewal that fails leaves the session running and retries later", () => {
	let writes = 0;
	const bus = {
		publish() {
			writes += 1;
			if (writes > 1) throw new Error("database or disk is full");
		},
	} as unknown as Bus;
	const presence = new Presence(bus, {
		address: "session/x",
		participant: "01a127d9",
		state: "idle",
	});

	assert.equal(presence.start(), true);
	assert.equal(presence.renew(), false, "the failed renewal is not fatal");
	assert.equal(presence.held, false);
	assert.match(String((presence.error as Error).message), /disk is full/);
	// The next renewal is attempted, not skipped.
	assert.equal(presence.renew(), false);
	assert.equal(writes, 3);
});
