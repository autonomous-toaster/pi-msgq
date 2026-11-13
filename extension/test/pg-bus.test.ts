// The shared engine's bus, against a real PostgreSQL with pgmq.
//
// These run when MSGQ_TEST_DATABASE_URI names a database, which `just test` provides. Without one
// they say so and return: a shared-engine test that never ran is not evidence, and the suite says
// which it was.

import assert from "node:assert/strict";
import { test } from "node:test";

import { PgBus, pgSettings } from "../src/pg-bus.ts";

const DSN = process.env.MSGQ_TEST_DATABASE_URI?.trim();

function unique(): string {
	return `${process.pid}-${Date.now()}-${Math.floor(Math.random() * 1_000_000)}`;
}

/** Publish straight to the engine, the way any other client would. */
async function publish(dsn: string, address: string, record: unknown): Promise<number> {
	const module = await import("pg");
	const client = new module.default.Client({ connectionString: dsn });
	await client.connect();
	try {
		const queue = await client.query(
			"SELECT msgq.ensure_queue(msgq.canonical_address($1)) AS name",
			[address],
		);
		const name = String(queue.rows[0].name);
		const sent = await client.query(
			`SELECT sent FROM pgmq.send($1, $2::text::jsonb, $3::text::jsonb) AS sent`,
			[
				name,
				JSON.stringify(record),
				JSON.stringify({ msgq: { address: address.toLowerCase() } }),
			],
		);
		await client.query(
			"UPDATE msgq.address SET newest_offset = GREATEST(newest_offset, $1) WHERE address = $2",
			[Number(sent.rows[0].sent), address.toLowerCase()],
		);
		return Number(sent.rows[0].sent);
	} finally {
		await client.end();
	}
}

test("the settings supply what the location does not", () => {
	assert.deepEqual(pgSettings("postgres://a@h:6000/db", {}), {
		connectionString: "postgres://a@h:6000/db",
	});
	assert.deepEqual(
		pgSettings("postgres://a@h/db", {
			PI_MSGQ_PGPASSWORD: "secret",
			PI_MSGQ_PGPORT: "55433",
			PI_MSGQ_PGUSER: "ignored",
		}),
		{ connectionString: "postgres://a@h/db", password: "secret", port: 55433, user: "ignored" },
	);
	// A location that names nothing takes everything from the environment, as the CLI does.
	assert.deepEqual(
		pgSettings("", {
			PI_MSGQ_PGHOST: "127.0.0.1",
			PI_MSGQ_PGDATABASE: "msgq",
			PI_MSGQ_PGSSLMODE: "disable",
		}),
		{ host: "127.0.0.1", database: "msgq" },
	);
});

test("a subscription receives what another client publishes", async (t) => {
	if (!DSN) {
		t.diagnostic("MSGQ_TEST_DATABASE_URI is unset: the shared bus was not exercised");
		return;
	}
	const address = `group/ext-${unique()}`;
	const consumer = `session/ext-${unique()}`;
	const bus = await PgBus.open(DSN, [address]);

	const events: unknown[] = [];
	let resolve: () => void = () => {};
	const seen = new Promise<void>((done) => {
		resolve = done;
	});
	const subscription = bus.subscribe(
		address,
		consumer,
		(event) => {
			events.push(event.payload);
			subscription.save(event.offset);
			resolve();
		},
		(error) => {
			throw error;
		},
	);
	try {
		await publish(DSN, address, { v: 1, type: "note", message: "hello" });
		await Promise.race([seen, new Promise((_, fail) => setTimeout(fail, 8000))]);
		assert.equal(events.length, 1);
		assert.deepEqual(events[0], { v: 1, type: "note", message: "hello" });
	} finally {
		subscription.close();
		await bus.close();
	}
});

test("a cursor moves forward and never back", async (t) => {
	if (!DSN) {
		t.diagnostic("MSGQ_TEST_DATABASE_URI is unset: the cursor was not exercised");
		return;
	}
	const address = `group/ext-cursor-${unique()}`;
	const consumer = `session/ext-cursor-${unique()}`;
	const bus = await PgBus.open(DSN, [address]);
	const module = await import("pg");
	const raw = new module.default.Client({ connectionString: DSN });
	await raw.connect();
	try {
		const offset = await publish(DSN, address, { v: 1, type: "note", message: "one" });
		const subscription = bus.subscribe(address, consumer, () => {}, (error) => {
			throw error;
		});
		subscription.save(offset);
		subscription.save(offset - 1);
		await new Promise((done) => setTimeout(done, 300));
		const stored = await raw.query(
			"SELECT last_msg_id FROM msgq.cursor WHERE consumer = $1 AND address = $2",
			[consumer, address],
		);
		assert.ok(stored.rows[0], "the consumer has a cursor");
		assert.ok(Number(stored.rows[0].last_msg_id) >= offset, "it only moves forward");
		subscription.close();
	} finally {
		await raw.end();
		await bus.close();
	}
});

test("lost records are reported, not hidden", async (t) => {
	if (!DSN) {
		t.diagnostic("MSGQ_TEST_DATABASE_URI is unset: the gap report was not exercised");
		return;
	}
	const address = `group/ext-gap-${unique()}`;
	const bus = await PgBus.open(DSN, [address]);
	const module = await import("pg");
	const raw = new module.default.Client({ connectionString: DSN });
	await raw.connect();
	try {
		await publish(DSN, address, { v: 1, type: "note", message: "old" });
		await raw.query(
			"INSERT INTO msgq.retention (topic, cut_offset, removed_total) VALUES ($1, 1, 1)",
			[address],
		);
		assert.equal(await bus.dropped(address, 0), 1, "a consumer behind the cut lost one");
		assert.equal(await bus.dropped(address, 1), 0, "a consumer at the cut lost nothing");
	} finally {
		await raw.end();
		await bus.close();
	}
});
