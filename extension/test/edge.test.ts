import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";

import { attachNotice, presenceReachable, statusFor } from "../src/attach.ts";
import type { AttachmentChange, AttachmentState } from "../src/consumer.ts";

test("the shared bus holds no direct select", async () => {
	const source = await readFile(new URL("../src/pg-bus.ts", import.meta.url), "utf8");
	const direct = source.match(/FROM\s+pgmq\./gi) ?? [];
	assert.deepEqual(direct, [], "a read must go through the function surface, not a queue table");
});

const ADDRESS = "session/worker";

function change(fields: Partial<AttachmentChange> & { state: "attached" | "retrying" }): AttachmentChange {
	return { failures: 1, ...fields };
}

test("a detachment is one notice, however many attempts follow", () => {
	const detached = attachNotice(change({ state: "retrying", reason: "database is locked" }), ADDRESS, "attached");
	assert.equal(detached?.kind, "detached");
	assert.match(detached?.content ?? "", /cannot read session\/worker: database is locked/);
	assert.match(detached?.content ?? "", /kept on the bus/);

	// The threshold notice is the same detachment said again: a toast, not a second entry.
	assert.equal(
		attachNotice(change({ state: "retrying", reason: "database is locked", repeat: true }), ADDRESS, "retrying"),
		undefined,
	);
});

test("recovery is a notice, and it comes before the records", () => {
	const recovered = attachNotice(change({ state: "attached", failures: 0 }), ADDRESS, "retrying");
	assert.equal(recovered?.kind, "attached");
	assert.match(recovered?.content ?? "", /reading session\/worker again/);

	const withBacklog = attachNotice(change({ state: "attached", failures: 0, waiting: 24 }), ADDRESS, "retrying");
	assert.match(withBacklog?.content ?? "", /24 records were waiting/);
});

test("a backlog is announced even when nothing was lost", () => {
	const first = attachNotice(change({ state: "attached", failures: 0, waiting: 1 }), ADDRESS, "attaching");
	assert.match(first?.content ?? "", /1 record were waiting|1 record/);
	assert.equal(
		attachNotice(change({ state: "attached", failures: 0, waiting: 0 }), ADDRESS, "attaching"),
		undefined,
		"an ordinary attach is not news",
	);
});

test("the status slot never claims a subscription that is not reading", () => {
	assert.equal(statusFor(["attached", "attached"]), "msgq: attached (2)");
	assert.equal(statusFor(["attached", "retrying"]), "msgq: detached (retrying)");
	assert.equal(statusFor(["attaching", "attached"]), "msgq: attaching (2)");
	assert.equal(statusFor([]), undefined);
});

test("a session that cannot read is not advertised as reachable", () => {
	const states: AttachmentState[][] = [[], ["attaching"], ["retrying"], ["attaching", "retrying"]];
	for (const set of states) {
		assert.equal(presenceReachable(set), false, `expected ${JSON.stringify(set)} to be unreachable`);
	}
	assert.equal(presenceReachable(["attached"]), true);
	assert.equal(presenceReachable(["retrying", "attached"]), true, "one address that reads is still reachable");
});

test("the extension wires the pump's state instead of a terminal disable", async () => {
	const source = await readFile(new URL("../msgq.ts", import.meta.url), "utf8");

	// The subscription hands its state to the session. It used to hand over an error that called
	// disable(), which ended delivery for the life of the process.
	assert.match(source, /\(change\) => onAttachState\(address, change\)/);
	assert.doesNotMatch(
		source,
		/disable\(`bus subscription failed/,
		"a subscription failure must not disable the extension",
	);

	// disable() survives for the one condition the pump cannot recover from: configuration.
	const calls = source.match(/disable\(/g) ?? [];
	assert.equal(calls.length, 2, "one definition plus one configuration call site");
	assert.match(source, /disable\(error instanceof ConfigError/);
});

test("a state notice is written into the transcript and starts no turn", async () => {
	const source = await readFile(new URL("../msgq.ts", import.meta.url), "utf8");
	const write = source.slice(source.indexOf("function writeNotice"), source.indexOf("function suspendPresence"));
	assert.match(write, /customType: MESSAGE_TYPE/);
	assert.match(write, /triggerTurn: false/, "a broken bus must not start a run");
});
