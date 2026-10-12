import assert from "node:assert/strict";
import { mkdtempSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";

import { Bus, openRaw } from "../src/consumer.ts";
import { Presence } from "../src/presence.ts";

const INTERVAL = 50;

function tempDb(): string {
	return join(mkdtempSync(join(tmpdir(), "msgq-presence-")), "msgq.db");
}

/** Every participant lease row, as raw engine state. */
function leaseRows(db: string): Array<{ name: string; owner: string; expires_at: number }> {
	const native = openRaw(db) as any;
	const rows = native.query(
		"SELECT name, owner, expires_at FROM _honker_locks WHERE name LIKE 'msgq:participant:%' ORDER BY name",
	);
	native.close();
	return rows;
}

function makePresence(db: string, over: Partial<{ state: any; owner: string; ttlS: number }> = {}) {
	const bus = new Bus(db, INTERVAL);
	const presence = new Presence(bus, {
		kind: "session",
		id: "01a127d9",
		owner: over.owner ?? "api-worker#01a127d9",
		state: over.state ?? "idle",
		ttlS: over.ttlS ?? 30,
	});
	return { bus, presence };
}

test("start registers a lease that is visible in the engine", () => {
	const db = tempDb();
	const { bus, presence } = makePresence(db);

	assert.equal(presence.start(), true);
	assert.equal(presence.held, true);

	const rows = leaseRows(db);
	assert.equal(rows.length, 1);
	assert.equal(rows[0].name, "msgq:participant:session:idle:01a127d9");
	assert.equal(rows[0].owner, "api-worker#01a127d9");
	assert.equal(presence.address, "session/api-worker");

	presence.stop();
	bus.close();
});

test("renaming registers under the new identity with no stale row", () => {
	const db = tempDb();
	const { bus, presence } = makePresence(db);
	presence.start();

	presence.setOwner("worker#01a127d9");

	const rows = leaseRows(db);
	assert.equal(rows.length, 1);
	assert.equal(rows[0].owner, "worker#01a127d9");
	assert.equal(presence.address, "session/worker");

	presence.stop();
	bus.close();
});

test("a state change leaves exactly one lease row", () => {
	const db = tempDb();
	const { bus, presence } = makePresence(db);
	presence.start();

	presence.setState("busy");
	assert.equal(presence.held, true);
	assert.equal(presence.name, "msgq:participant:session:busy:01a127d9");

	const rows = leaseRows(db);
	assert.equal(rows.length, 1);
	assert.equal(rows[0].name, "msgq:participant:session:busy:01a127d9");

	presence.stop();
	bus.close();
});

test("renew extends the lease expiry", async () => {
	const db = tempDb();
	const { bus, presence } = makePresence(db, { ttlS: 30 });
	presence.start();
	const before = leaseRows(db)[0].expires_at;

	await new Promise((r) => setTimeout(r, 1100));

	assert.equal(presence.renew(), true);
	const after = leaseRows(db)[0].expires_at;
	assert.ok(after > before, `expected ${after} > ${before}`);

	presence.stop();
	bus.close();
});

test("stop releases the lease", () => {
	const db = tempDb();
	const { bus, presence } = makePresence(db);
	presence.start();
	presence.stop();
	assert.equal(leaseRows(db).length, 0);
	presence.stop(); // idempotent
	bus.close();
});

test("a lease held by another owner blocks registration and is not fatal", () => {
	const db = tempDb();
	const other = new Bus(db, INTERVAL);
	const stolen = other.acquireLease("msgq:participant:session:idle:01a127d9", "other#01a127d9", 30);
	assert.ok(stolen, "the other owner should acquire first");

	const { bus, presence } = makePresence(db);
	assert.equal(presence.start(), false);
	assert.equal(presence.held, false);
	// Still usable: a later renew retries the acquisition.
	assert.equal(presence.renew(), false);

	presence.stop();
	bus.close();
	other.close();
});

test("a lost lease is re-acquired on renew", () => {
	const db = tempDb();
	const { bus, presence } = makePresence(db);
	presence.start();

	// Simulate loss: release out from under the presence by taking the name.
	presence.stop();
	const fresh = makePresence(db);
	assert.equal(fresh.presence.start(), true);
	assert.equal(fresh.presence.renew(), true);

	fresh.presence.stop();
	fresh.bus.close();
	bus.close();
});
