import assert from "node:assert/strict";
import { test } from "node:test";

import { Coalescer, formatBatch, formatMessage, planDelivery } from "../src/delivery.ts";
import type { MsgqRecord } from "../src/record.ts";

const SELF = "session/api-worker";

function record(over: Partial<MsgqRecord> = {}): MsgqRecord {
	return {
		v: 1,
		type: "prompt",
		from: "cli/alice@mac",
		to: SELF,
		message: "hello",
		...over,
	};
}

test("display policy downgrades every record", () => {
	assert.equal(planDelivery(record({ type: "prompt" }), "display", SELF, false), "display");
	assert.equal(planDelivery(record({ type: "steer" }), "display", SELF, true), "display");
});

test("honor policy maps turn-bearing types by agent state", () => {
	assert.equal(planDelivery(record({ type: "prompt" }), "honor", SELF, false), "turn");
	assert.equal(planDelivery(record({ type: "prompt" }), "honor", SELF, true), "buffer");
	assert.equal(planDelivery(record({ type: "steer" }), "honor", SELF, false), "turn");
	assert.equal(planDelivery(record({ type: "steer" }), "honor", SELF, true), "buffer");
	assert.equal(planDelivery(record({ type: "follow_up" }), "honor", SELF, false), "turn");
	assert.equal(planDelivery(record({ type: "follow_up" }), "honor", SELF, true), "buffer");
});

test("note and unknown types never turn", () => {
	assert.equal(planDelivery(record({ type: "note" }), "honor", SELF, false), "display");
	assert.equal(planDelivery(record({ type: "future-kind" }), "honor", SELF, false), "display");
});

test("a self-addressed record never starts a turn", () => {
	assert.equal(planDelivery(record({ from: SELF }), "honor", SELF, false), "display");
	assert.equal(planDelivery(record({ from: SELF }), "honor", SELF, true), "display");
});

test("a record with no body is display-only", () => {
	assert.equal(
		planDelivery(record({ type: "prompt", message: undefined }), "honor", SELF, false),
		"display",
	);
});

test("messages carry provenance", () => {
	const text = formatMessage(record(), "group/a");
	assert.match(text, /cli\/alice@mac/);
	assert.match(text, /group\/a/);
	assert.match(text, /hello/);
});

test("coalescer drains and empties", () => {
	const coalescer = new Coalescer();
	assert.equal(coalescer.empty, true);
	coalescer.add(record({ message: "one" }), "group/a");
	coalescer.add(record({ message: "two" }), "group/a");
	assert.equal(coalescer.size, 2);
	assert.equal(coalescer.empty, false);

	const drained = coalescer.drain();
	assert.equal(drained.length, 2);
	assert.equal(coalescer.empty, true);
	assert.equal(coalescer.size, 0);
});

test("batch rendering lists every record with provenance", () => {
	const text = formatBatch([
		{ record: record({ message: "one", from: "cli/a@h" }), address: "group/a" },
		{ record: record({ message: "two", from: "cli/b@h" }), address: "group/a" },
	]);
	assert.match(text, /2 messages/);
	assert.match(text, /cli\/a@h/);
	assert.match(text, /cli\/b@h/);
	assert.match(text, /one/);
	assert.match(text, /two/);
});
