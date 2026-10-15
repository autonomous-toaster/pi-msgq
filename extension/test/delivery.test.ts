import assert from "node:assert/strict";
import { test } from "node:test";

import {
	Coalescer,
	formatBatch,
	formatCommandNote,
	formatMessage,
	formatOwed,
	formatReference,
	planDelivery,
	wantsReply,
} from "../src/delivery.ts";
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

test("a trusted sender allowlist gates the honour policy", () => {
	// Named apart from the helper, which would otherwise shadow it here.
	const fromPlanner = record({ from: "session/planner" });
	// A configured list admits only its members.
	assert.equal(planDelivery(fromPlanner, "honor", SELF, false, ["session/planner"]), "turn");
	assert.equal(planDelivery(fromPlanner, "honor", SELF, false, ["session/other"]), "display");
	// An empty list means no allowlist was configured, so nothing is gated.
	assert.equal(planDelivery(fromPlanner, "honor", SELF, false, []), "turn");
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

test("a record asking for an answer carries a reply hint", () => {
	const asking = record({ message: "what is 2+2?", meta: { reply: true } });
	assert.match(formatMessage(asking, "group/a"), /peers tool/);
	assert.equal(wantsReply(asking), true);

	const plain = record({ message: "just a note" });
	assert.equal(wantsReply(plain), false);
	assert.doesNotMatch(formatMessage(plain, "group/a"), /peers tool/);
});

test("a reference names the record it refers to, and quotes it", () => {
	assert.equal(
		formatReference({
			id: "session/reviewer#7",
			record: record({ from: "session/planner", message: "review this diff" }),
		}),
		'[about session/reviewer#7 from session/planner: "review this diff"]',
	);
});

test("a reference that cannot be read still names the id", () => {
	// The reader can at least tell that something is being answered.
	assert.equal(formatReference({ id: "session/reviewer#7" }), "[about session/reviewer#7]");
});

test("a referenced record with no body is named without a quote", () => {
	assert.equal(
		formatReference({
			id: "cli/x#3",
			record: record({ from: "cli/x", type: "event", message: undefined }),
		}),
		"[about cli/x#3 from cli/x]",
	);
});

test("a quoted body stays on one line", () => {
	assert.equal(
		formatReference({
			id: "cli/x#3",
			record: record({ from: "cli/x", message: "one\n   two" }),
		}),
		'[about cli/x#3 from cli/x: "one two"]',
	);
});

test("messages carry provenance", () => {
	const text = formatMessage(record(), "group/a");
	assert.match(text, /cli\/alice@mac/);
	assert.match(text, /group\/a/);
	assert.match(text, /hello/);
});

test("the content never carries the human's reply affordance", () => {
	// The content is what the model reads. How a person answers belongs to the
	// renderer, and must never leak into this.
	for (const text of [
		formatMessage(record(), "group/a"),
		formatMessage(record({ message: "what is 2+2?", meta: { reply: true } }), "group/a"),
		formatBatch([{ record: record({ message: "one" }), address: "group/a" }]),
	]) {
		assert.doesNotMatch(text, /\/peer/);
		assert.doesNotMatch(text, /reply:/);
	}
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

test("the entry for an owed answer names the question and both ways out", () => {
	// Declining has to be offered, or the entry teaches that silence is a bug.
	const text = formatOwed({ from: "session/planner", id: "session/planner#7" });
	assert.match(text, /session\/planner asked you a question \(session\/planner#7\)/);
	assert.match(text, /peers tool/);
	assert.match(text, /nothing needed reporting/);
	assert.match(text, /Do not simply stop/);
});

test("a command is planned for dispatch, never for a batch", () => {
	const command = record({ type: "command", message: "/harsh-review the parser" });
	assert.equal(planDelivery(command, "honor", SELF, false), "dispatch");
	// Even mid-run: a batch is one message, and a command has to be the whole text.
	assert.equal(planDelivery(command, "honor", SELF, true), "dispatch");
});

test("a command is gated exactly like anything else that acts", () => {
	const command = record({ type: "command", message: "/harsh-review" });
	assert.equal(planDelivery(command, "display", SELF, false), "display");
	assert.equal(planDelivery({ ...command, from: SELF }, "honor", SELF, false), "display");
	assert.equal(
		planDelivery(command, "honor", SELF, false, ["session/parent"]),
		"display",
		"an untrusted sender's command must not run",
	);
	assert.equal(
		planDelivery({ ...command, message: "" }, "honor", SELF, false),
		"display",
	);
});

test("the note before a command says who is speaking", () => {
	const text = formatCommandNote(record({ type: "command", from: "session/planner" }), "session/reviewer");
	assert.match(text, /\[msgq\] from session\/planner on session\/reviewer/);
	// The distinction provenance exists for: the next message is not the user's.
	assert.match(text, /not from your user/);
	// The body itself is not repeated here; it is the command.
	assert.doesNotMatch(text, /hello/);
});

test("the note before a command carries a reference too", () => {
	const text = formatCommandNote(record({ from: "session/planner" }), "session/reviewer", {
		id: "session/planner#4",
		record: record({ from: "session/reviewer", message: "run this" }),
	});
	assert.match(text, /\[about session\/planner#4 from session\/reviewer: "run this"\]/);
});
