import assert from "node:assert/strict";
import { test } from "node:test";

import { decodeRecord, isKnownType, recordBody } from "../src/record.ts";

test("decodes a well-formed record", () => {
	const result = decodeRecord({
		v: 1,
		type: "prompt",
		from: "cli/alice@mac",
		to: "session/api-worker",
		message: "deploy finished",
	});
	assert.equal(result.ok, true);
	if (result.ok) {
		assert.equal(result.record.type, "prompt");
		assert.equal(result.record.from, "cli/alice@mac");
		assert.equal(recordBody(result.record), "deploy finished");
	}
});

test("tolerates an unknown type and preserves unknown fields", () => {
	const result = decodeRecord({
		v: 1,
		type: "future-kind",
		from: "cli/x",
		to: "group/a",
		message: "hi",
		custom: { k: 1 },
	});
	assert.equal(result.ok, true);
	if (result.ok) {
		assert.equal(isKnownType(result.record.type), false);
		assert.deepEqual(result.record.custom, { k: 1 });
	}
});

test("rejects a missing required field", () => {
	for (const payload of [
		{ type: "note", from: "a", to: "b" },
		{ v: 1, from: "a", to: "b" },
		{ v: 1, type: "note", to: "b" },
		{ v: 1, type: "note", from: "a" },
	]) {
		const result = decodeRecord(payload);
		assert.equal(result.ok, false);
	}
});

test("skips an unsupported protocol version", () => {
	const result = decodeRecord({ v: 2, type: "note", from: "a", to: "b" });
	assert.equal(result.ok, false);
	if (!result.ok) assert.match(result.reason, /version/);
});

test("skips payloads that are not objects", () => {
	for (const payload of [null, undefined, "text", 42, [1, 2]]) {
		const result = decodeRecord(payload);
		assert.equal(result.ok, false);
	}
});

test("rejects a non-string message", () => {
	const result = decodeRecord({ v: 1, type: "note", from: "a", to: "b", message: 5 });
	assert.equal(result.ok, false);
});

test("recordBody summarizes an event record", () => {
	const result = decodeRecord({
		v: 1,
		type: "event",
		from: "a",
		to: "b",
		event: { type: "agent_settled" },
	});
	assert.equal(result.ok, true);
	if (result.ok) assert.match(recordBody(result.record), /agent_settled/);
});
