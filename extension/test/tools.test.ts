import assert from "node:assert/strict";
import { test } from "node:test";

import {
	type Command,
	DEFAULT_ASK_TIMEOUT_S,
	isError,
	planPeerAction,
} from "../src/tools.ts";

const peers = {
	msgqBin: "msgq",
	self: "session/planner",
	lastInbound: { from: "session/reviewer", offset: 42 },
	expectCompletion: true,
};

/** The same session, sending something that did not ask to be told how it ended. */
const quiet = { ...peers, expectCompletion: false };

function cmd(plan: Command | { error: string }): Command {
	assert.equal(isError(plan), false, `unexpected error: ${JSON.stringify(plan)}`);
	return plan as Command;
}

test("list asks the bus who is reachable", () => {
	assert.deepEqual(cmd(planPeerAction({ action: "list" }, peers)), {
		command: "msgq",
		args: ["sessions"],
	});
});

test("send publishes a turn-bearing message as this session", () => {
	assert.deepEqual(cmd(planPeerAction({ action: "send", to: "session/x", message: "hi" }, peers)), {
		command: "msgq",
		args: [
			"publish",
			"session/x",
			"hi",
			"--type",
			"prompt",
			"--from",
			"session/planner",
			"--ack",
			"completed",
		],
	});
});

test("send asks to be told when the peer's run completes", () => {
	// A sender that chose not to wait still learns the peer finished, including when it
	// finished without answering.
	const args = cmd(planPeerAction({ action: "send", to: "session/x", message: "hi" }, peers)).args;
	assert.deepEqual(args.slice(-2), ["--ack", "completed"]);
});

test("a send that did not ask is left alone", () => {
	// The whole planner is shared with the human's own commands, and a completion report is
	// for a delegation rather than for every message a person types.
	assert.deepEqual(cmd(planPeerAction({ action: "send", to: "session/x", message: "hi" }, quiet)).args, [
		"publish",
		"session/x",
		"hi",
		"--type",
		"prompt",
		"--from",
		"session/planner",
	]);
});

test("sending to itself never asks to be told how it ended", () => {
	// Nothing is being waited on, and the acknowledgement would come straight back.
	assert.equal(
		cmd(planPeerAction({ action: "send", to: "session/planner", message: "hi" }, peers)).args.includes("--ack"),
		false,
	);
});

test("asking and replying are unchanged", () => {
	// An ask already requests the whole lifecycle; a reply is an answer, not a request.
	assert.equal(
		cmd(planPeerAction({ action: "ask", to: "session/x", message: "hi" }, peers)).args.includes("--ack"),
		false,
	);
	assert.equal(
		cmd(planPeerAction({ action: "reply", message: "ok" }, peers)).args.includes("--ack"),
		false,
	);
});

test("send without a recipient or a message is refused", () => {
	assert.ok(isError(planPeerAction({ action: "send", message: "hi" }, peers)));
	assert.ok(isError(planPeerAction({ action: "send", to: "session/x" }, peers)));
});

test("ask carries a wait window, defaulting generously", () => {
	assert.deepEqual(
		cmd(planPeerAction({ action: "ask", to: "session/x", message: "?" }, peers)).args,
		["ask", "session/x", "?", "--timeout", String(DEFAULT_ASK_TIMEOUT_S), "--from", "session/planner"],
	);
	assert.deepEqual(
		cmd(planPeerAction({ action: "ask", to: "session/x", message: "?", timeoutSeconds: 30 }, peers))
			.args,
		["ask", "session/x", "?", "--timeout", "30", "--from", "session/planner"],
	);
	assert.ok(isError(planPeerAction({ action: "ask", message: "?" }, peers)));
});

test("reply targets the sender of the last received message", () => {
	assert.deepEqual(cmd(planPeerAction({ action: "reply", message: "done" }, peers)).args, [
		"publish",
		"session/reviewer",
		"done",
		"--type",
		"prompt",
		"--reply-to",
		"42",
		"--from",
		"session/planner",
	]);
});

test("reply may name a recipient but keeps the reply reference", () => {
	assert.deepEqual(
		cmd(planPeerAction({ action: "reply", to: "session/other", message: "done" }, peers)).args,
		[
			"publish",
			"session/other",
			"done",
			"--type",
			"prompt",
			"--reply-to",
			"42",
			"--from",
			"session/planner",
		],
	);
});

test("every outbound peer action speaks as this session", () => {
	for (const params of [
		{ action: "send", to: "session/x", message: "hi" },
		{ action: "ask", to: "session/x", message: "?" },
		{ action: "reply", message: "done" },
	] as const) {
		const args = cmd(planPeerAction(params, peers)).args;
		assert.ok(
			args.includes("--from") && args[args.indexOf("--from") + 1] === "session/planner",
			`missing --from in ${args.join(" ")}`,
		);
	}
});

test("reply without anything to reply to is refused", () => {
	assert.ok(
		isError(
			planPeerAction({ action: "reply", message: "done" }, { ...peers, lastInbound: undefined }),
		),
	);
	assert.ok(isError(planPeerAction({ action: "reply" }, peers)));
});

test("unknown peer actions are refused", () => {
	assert.ok(isError(planPeerAction({ action: "shout" } as never, peers)));
});

test("acting without a bus address is refused", () => {
	const anonymous = { ...peers, self: "" };
	for (const params of [
		{ action: "send", to: "session/x", message: "hi" },
		{ action: "ask", to: "session/x", message: "?" },
		{ action: "reply", message: "done" },
	] as const) {
		assert.ok(isError(planPeerAction(params, anonymous)), JSON.stringify(params));
	}
	// Listing uses no identity, so it still works.
	assert.equal(isError(planPeerAction({ action: "list" }, anonymous)), false);
});
