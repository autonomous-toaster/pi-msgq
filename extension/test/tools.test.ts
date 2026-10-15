import assert from "node:assert/strict";
import { test } from "node:test";

import {
	type Command,
	DEFAULT_ASK_TIMEOUT_S,
	isError,
	planCompanionAction,
	planPeerAction,
} from "../src/tools.ts";

const peers = {
	msgqBin: "msgq",
	self: "session/planner",
	reportTo: "session/planner",
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

test("send falls back to the reporting address", () => {
	const plan = cmd(
		planPeerAction({ action: "send", message: "hi" }, { ...peers, reportTo: "session/parent" }),
	);
	assert.deepEqual(plan.args, [
		"publish",
		"session/parent",
		"hi",
		"--type",
		"prompt",
		"--from",
		"session/planner",
		"--ack",
		"completed",
	]);
});

test("a fallback that lands on this session does not ask to be told either", () => {
	assert.equal(
		cmd(planPeerAction({ action: "send", message: "hi" }, peers)).args.includes("--ack"),
		false,
	);
});

test("send without a recipient or a message is refused", () => {
	assert.ok(
		isError(planPeerAction({ action: "send", message: "hi" }, { ...peers, reportTo: undefined })),
	);
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
	assert.ok(isError(planPeerAction({ action: "ask", message: "?" }, { ...peers, reportTo: undefined })));
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
	// A companion spawned with no parent would have no trusted-sender allowlist at all.
	const anonymous = { ...peers, self: "" };
	for (const params of [
		{ action: "send", to: "session/x", message: "hi" },
		{ action: "ask", to: "session/x", message: "?" },
		{ action: "reply", message: "done" },
	] as const) {
		assert.ok(isError(planPeerAction(params, anonymous)), JSON.stringify(params));
	}
	assert.ok(
		isError(planCompanionAction({ action: "spawn", name: "x" }, { companionBin: "companion", self: "" })),
	);
	// Listing and closing use no identity, so they still work.
	assert.equal(isError(planPeerAction({ action: "list" }, anonymous)), false);
	assert.equal(
		isError(planCompanionAction({ action: "close", name: "x" }, { companionBin: "companion", self: "" })),
		false,
	);
});

test("spawn points the companion at this session", () => {
	assert.deepEqual(
		cmd(
			planCompanionAction(
				{ action: "spawn", name: "reviewer" },
				{ companionBin: "companion", self: "session/planner" },
			),
		),
		{
			command: "companion",
			args: ["spawn", "reviewer", "--parent", "session/planner", "--json"],
		},
	);
	assert.deepEqual(
		cmd(
			planCompanionAction(
				{ action: "spawn", name: "reviewer", cwd: "/tmp/repo" },
				{ companionBin: "companion", self: "session/planner" },
			),
		).args,
		["spawn", "reviewer", "--parent", "session/planner", "--json", "--cwd", "/tmp/repo"],
	);
	assert.ok(
		isError(
			planCompanionAction(
				{ action: "spawn" },
				{ companionBin: "companion", self: "session/planner" },
			),
		),
	);
});

test("close names the companion", () => {
	assert.deepEqual(
		cmd(planCompanionAction({ action: "close", name: "reviewer" }, { companionBin: "companion", self: "x" }))
			.args,
		["close", "reviewer"],
	);
	assert.ok(
		isError(planCompanionAction({ action: "close" }, { companionBin: "companion", self: "x" })),
	);
});

test("a spawn trusts the parent alone by default", () => {
	assert.deepEqual(
		cmd(planCompanionAction({ action: "spawn", name: "reviewer" }, { companionBin: "companion", self: "session/p" })).args,
		["spawn", "reviewer", "--parent", "session/p", "--json"],
	);
});

test("a spawn widens the trusted set when asked", () => {
	const plan = cmd(
		planCompanionAction(
			{ action: "spawn", name: "reviewer", accept: "cli/second@test,group/family" },
			{ companionBin: "companion", self: "session/p" },
		),
	);
	assert.deepEqual(plan.args, [
		"spawn",
		"reviewer",
		"--parent",
		"session/p",
		"--json",
		"--accept",
		"cli/second@test,group/family",
	]);
});

test("a spawn carries the working directory and the trusted set together", () => {
	const plan = cmd(
		planCompanionAction(
			{ action: "spawn", name: "reviewer", cwd: "/src/api", accept: "cli/ci@host" },
			{ companionBin: "companion", self: "session/p" },
		),
	);
	assert.deepEqual(plan.args.slice(-4), ["--cwd", "/src/api", "--accept", "cli/ci@host"]);
});

test("send can carry a command instead of a message", () => {
	const plan = cmd(
		planPeerAction({ action: "send", to: "session/x", message: "/harsh-review the parser", command: true }, peers),
	);
	assert.deepEqual(plan.args, [
		"publish",
		"session/x",
		"/harsh-review the parser",
		"--type",
		"command",
		"--from",
		"session/planner",
		"--ack",
		"completed",
	]);
});

test("asking a command is refused, and the refusal names the fix", () => {
	const plan = planPeerAction({ action: "ask", to: "session/x", message: "/harsh-review", command: true }, peers);
	assert.equal(isError(plan), true);
	assert.match(
		(plan as { error: string }).error,
		/put the request for an answer in its own arguments/,
	);
});

test("only sending takes a command", () => {
	const plan = planPeerAction({ action: "reply", message: "ok", command: true }, peers);
	assert.equal(isError(plan), true);
	assert.match((plan as { error: string }).error, /only 'send' takes a command/);
});
