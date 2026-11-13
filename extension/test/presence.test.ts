import assert from "node:assert/strict";
import { mkdtempSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";

import { HonkerBus, openRaw, type Bus } from "../src/consumer.ts";
import { PRESENCE_ADDRESS, Presence, PRESENCE_TTL_S, presenceAddress } from "../src/presence.ts";
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
	over: Partial<{
		state: any;
		alias: string;
		address: string;
		participant: string;
		ttlS: number;
		card: Record<string, string>;
	}> = {},
) {
	const bus = new HonkerBus(db, INTERVAL);
	const presence = new Presence(bus, {
		address: over.address ?? "session/api-worker",
		participant: over.participant ?? "01a127d9",
		alias: "alias" in over ? over.alias : "api-worker",
		state: over.state ?? "idle",
		ttlS: over.ttlS ?? 30,
		card: over.card,
	});
	return { bus, presence };
}

test("start publishes one presence record", async () => {
	const db = tempDb();
	const { bus, presence } = makePresence(db);

	assert.equal(await presence.start(), true);
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

	await presence.stop();
	bus.close();
});

test("the presence record carries a locator card", async () => {
	const db = tempDb();
	const card = { project: "github.com/org/repo", branch: "main", path: "src" };
	const { bus, presence } = makePresence(db, { card });

	await presence.start();
	const [record] = presenceRecords(db);
	assert.deepEqual(presenceEvent(record).card, card);

	await presence.setCard({ ...card, branch: "review" });
	const records = presenceRecords(db);
	assert.equal(records.length, 2);
	assert.equal((presenceEvent(records[1]).card as Record<string, string>).branch, "review");

	// Setting the same card again publishes nothing new.
	assert.equal(await presence.setCard({ ...card, branch: "review" }), true);
	assert.equal(presenceRecords(db).length, 2);

	presence.setCard(undefined);
	const last = presenceRecords(db);
	assert.equal(presenceEvent(last[last.length - 1]).card, undefined);

	await presence.stop();
	bus.close();
});

test("a participant with no alias publishes without one", async () => {
	const db = tempDb();
	const { bus, presence } = makePresence(db, { alias: undefined, address: "session/01a127d9" });

	await presence.start();
	const [record] = presenceRecords(db);
	assert.equal(presenceEvent(record).alias, undefined);
	assert.equal(record.from, "session/01a127d9");

	await presence.stop();
	bus.close();
});

test("renew publishes a newer record rather than editing the old one", async () => {
	const db = tempDb();
	const { bus, presence } = makePresence(db);
	await presence.start();
	presence.renew();

	const records = presenceRecords(db);
	assert.equal(records.length, 2, "each renewal is a new record, so the newest one is the truth");

	await presence.stop();
	bus.close();
});

test("a state change is published promptly", async () => {
	const db = tempDb();
	const { bus, presence } = makePresence(db);
	await presence.start();
	assert.equal(await presence.setState("busy"), true);

	const records = presenceRecords(db);
	assert.equal(records.length, 2);
	assert.equal(presenceEvent(records[1]).state, "busy");
	// Setting the same state again publishes nothing new.
	assert.equal(await presence.setState("busy"), true);
	assert.equal(presenceRecords(db).length, 2);

	await presence.stop();
	bus.close();
});

test("a rename publishes under the new identity", async () => {
	const db = tempDb();
	const { bus, presence } = makePresence(db);
	await presence.start();

	assert.equal(await presence.setOwner({ address: "session/reviewer", alias: "reviewer" }), true);
	const records = presenceRecords(db);
	assert.equal(records.length, 2);
	assert.equal(records[1].from, "session/reviewer");
	assert.equal(presenceEvent(records[1]).alias, "reviewer");

	await presence.stop();
	bus.close();
});

test("stop retracts presence, and nothing is published afterwards", async () => {
	const db = tempDb();
	const { bus, presence } = makePresence(db);
	await presence.start();
	await presence.stop();

	const records = presenceRecords(db);
	const last = records[records.length - 1];
	assert.equal(presenceEvent(last).state, "gone");
	assert.equal(presence.held, false);
	assert.equal(await presence.start(), false, "a retracted presence is closed");
	assert.equal(await presence.renew(), false);

	await presence.stop(); // idempotent
	assert.equal(presenceRecords(db).length, records.length);

	bus.close();
});

test("the default lifetime is used when none is given", async () => {
	const db = tempDb();
	const bus = new HonkerBus(db, INTERVAL);
	const presence = new Presence(bus, {
		address: "session/x",
		participant: "01a127d9",
		state: "idle",
	});

	await presence.start();
	assert.equal(presenceEvent(presenceRecords(db)[0]).ttl, PRESENCE_TTL_S);

	await presence.stop();
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

test("a publish that fails is not fatal and keeps its reason", async () => {
	const presence = new Presence(failingBus(), {
		address: "session/x",
		participant: "01a127d9",
		state: "idle",
	});

	assert.equal(await presence.start(), false);
	assert.equal(presence.held, false, "a presence that was not published is not claimed");
	assert.match(String((presence.error as Error).message), /disk is full/);
});

test("a renewal that fails leaves the session running and retries later", async () => {
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

	assert.equal(await presence.start(), true);
	assert.equal(await presence.renew(), false, "the failed renewal is not fatal");
	assert.equal(presence.held, false);
	assert.match(String((presence.error as Error).message), /disk is full/);
	// The next renewal is attempted, not skipped.
	assert.equal(await presence.renew(), false);
	assert.equal(writes, 3);
});

test("presence is per scope, because the roster is", async () => {
	assert.equal(presenceAddress(), "group/presence");
	assert.equal(presenceAddress(""), "group/presence");
	assert.equal(presenceAddress("red"), "group/red/presence");
	assert.equal(presenceAddress(" RED/ "), "group/red/presence", "the address grammar decides");
});

test("a scoped session publishes to its own scope address", async () => {
	const db = tempDb();
	const bus = new HonkerBus(db);
	const presence = new Presence(bus, {
		address: "session/red-worker",
		participant: "01red",
		state: "idle",
		scope: "red",
	});
	assert.equal(await presence.start(), true);
	await presence.stop();

	const native = openRaw(db) as any;
	const topics = native.query("SELECT DISTINCT topic FROM _honker_stream ORDER BY topic", []);
	native.close();
	const rows = topics.map((row: { topic: string }) => row.topic);
	assert.deepEqual(rows, ["group/red/presence"]);
});
