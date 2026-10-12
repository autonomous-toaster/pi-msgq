import assert from "node:assert/strict";
import { test } from "node:test";

import {
	ackId,
	buildAck,
	isAckRecord,
	isTerminalAck,
	parseAckRequest,
	publishSafely,
} from "../src/ack.ts";

test("builds a received acknowledgement carrying the delivery mode", () => {
	const ack = buildAck({
		from: "session/child",
		to: "session/planner",
		replyTo: 42,
		phase: "received",
		delivery: "display",
	});
	assert.equal(ack.v, 1);
	assert.equal(ack.type, "event");
	assert.equal(ack.from, "session/child");
	assert.equal(ack.to, "session/planner");
	assert.equal(ack.reply_to, 42);
	assert.deepEqual(ack.event, { kind: "ack", phase: "received", delivery: "display" });
});

test("builds a completed acknowledgement carrying the outcome", () => {
	const ack = buildAck({
		from: "session/child",
		to: "session/planner",
		replyTo: 7,
		phase: "completed",
		outcome: "aborted",
	});
	assert.deepEqual(ack.event, { kind: "ack", phase: "completed", outcome: "aborted" });
});

test("a started acknowledgement carries neither delivery nor outcome", () => {
	const ack = buildAck({ from: "a", to: "b", replyTo: 1, phase: "started" });
	assert.deepEqual(ack.event, { kind: "ack", phase: "started" });
});

test("recognises an acknowledgement record", () => {
	assert.equal(isAckRecord({ type: "event", event: { kind: "ack" } }), true);
	assert.equal(isAckRecord({ type: "event", event: { kind: "other" } }), false);
	assert.equal(isAckRecord({ type: "note", message: "hi" }), false);
	assert.equal(isAckRecord({ type: "event", event: null }), false);
});

test("a true request asks for every phase", () => {
	assert.deepEqual(parseAckRequest({ ack: true }), ["received", "started", "completed"]);
});

test("a string request names a subset, trimmed and de-duplicated", () => {
	assert.deepEqual(parseAckRequest({ ack: "started, completed" }), ["started", "completed"]);
	assert.deepEqual(parseAckRequest({ ack: "received,received" }), ["received"]);
});

test("an unrecognised or absent request asks for nothing", () => {
	assert.equal(parseAckRequest(undefined), undefined);
	assert.equal(parseAckRequest({}), undefined);
	assert.equal(parseAckRequest({ ack: false }), undefined);
	assert.equal(parseAckRequest({ ack: "bogus" }), undefined);
	assert.equal(parseAckRequest({ ack: 3 }), undefined);
	assert.equal(parseAckRequest("not-an-object"), undefined);
});

test("acknowledgement identity is offset plus phase", () => {
	assert.equal(ackId(42, "completed"), "42:completed");
});

test("a failed acknowledgement publish is reported and does not throw", () => {
	const seen: unknown[] = [];
	assert.doesNotThrow(() =>
		publishSafely(
			() => {
				throw new Error("bus is gone");
			},
			(error) => seen.push(error),
		),
	);
	assert.equal(seen.length, 1);
	assert.match(String(seen[0]), /bus is gone/);
});

test("a successful acknowledgement publish reports nothing", () => {
	let published = 0;
	let reported = 0;
	publishSafely(
		() => {
			published += 1;
		},
		() => {
			reported += 1;
		},
	);
	assert.equal(published, 1);
	assert.equal(reported, 0);
});

test("terminal phases end the lifecycle", () => {
	assert.equal(isTerminalAck("completed"), true);
	assert.equal(isTerminalAck("received", "display"), true);
	assert.equal(isTerminalAck("received", "turn"), false);
	assert.equal(isTerminalAck("received", "queued"), false);
	assert.equal(isTerminalAck("started"), false);
});
