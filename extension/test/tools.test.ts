import assert from "node:assert/strict";
import { test } from "node:test";

import {
	askWaitSeconds,
	type Command,
	DEFAULT_ASK_TIMEOUT_S,
	interpretAsk,
	isError,
	MAX_ASK_TIMEOUT_S,
	PLAIN_LIST,
	planPeerAction,
	shouldRetryPlainList,
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
		args: ["sessions", "--wide"],
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

test("ask publishes and returns, so no turn is held open", () => {
	assert.deepEqual(cmd(planPeerAction({ action: "ask", to: "session/x", message: "?" }, peers)).args, [
		"ask",
		"session/x",
		"?",
		"--from",
		"session/planner",
		"--no-wait",
	]);
	assert.ok(isError(planPeerAction({ action: "ask", message: "?" }, peers)));
	// A wait window is meaningless without a wait, and must not sneak one in.
	assert.deepEqual(
		cmd(planPeerAction({ action: "ask", to: "session/x", message: "?", timeoutSeconds: 30 }, peers))
			.args,
		["ask", "session/x", "?", "--from", "session/planner", "--no-wait"],
	);
});

test("a waiting ask is bounded and machine-readable", () => {
	assert.deepEqual(
		cmd(planPeerAction({ action: "ask", to: "session/x", message: "?", wait: true }, peers)).args,
		[
			"ask",
			"session/x",
			"?",
			"--timeout",
			String(DEFAULT_ASK_TIMEOUT_S),
			"--from",
			"session/planner",
			"--json",
		],
	);
});

test("the model cannot ask for a wait without a limit", () => {
	// The CLI keeps --timeout 0 for scripts; the tool must never hand it over.
	assert.equal(askWaitSeconds(0), 1);
	assert.equal(askWaitSeconds(-5), 1);
	assert.equal(askWaitSeconds(30), 30);
	assert.equal(askWaitSeconds(5000), MAX_ASK_TIMEOUT_S);
	assert.equal(askWaitSeconds(Number.NaN), DEFAULT_ASK_TIMEOUT_S);
	assert.equal(askWaitSeconds(undefined), DEFAULT_ASK_TIMEOUT_S);
	assert.deepEqual(
		cmd(planPeerAction({ action: "ask", to: "session/x", message: "?", wait: true, timeoutSeconds: 0 }, peers))
			.args,
		["ask", "session/x", "?", "--timeout", "1", "--from", "session/planner", "--json"],
	);
});

test("a question to this session is refused", () => {
	// Nothing delivers a record back to its sender as a turn, so it could only time out.
	for (const to of ["session/planner", "me"]) {
		assert.ok(isError(planPeerAction({ action: "ask", to, message: "?" }, peers)), to);
	}
});

test("a wait's two endings are told apart by shape, not by prose", () => {
	// A record arrives wrapped: its position and its verdict sit beside it, never inside it.
	const answer = interpretAsk(
		{
			ok: true,
			text: JSON.stringify({
				topic: "session/planner",
				offset: 3,
				created_at: 1,
				verdict: { state: "trusted", principal: "planner", fingerprint: "SHA256:x" },
				record: { v: 1, type: "prompt", from: "session/x", to: "session/planner", message: "four" },
			}),
		},
		"session/x",
	);
	assert.equal(answer.answered, true);
	assert.equal(answer.text, "four");

	// An unpinned sender still answers; the model is told the answer is unverified.
	const stranger = interpretAsk(
		{
			ok: true,
			text: JSON.stringify({
				topic: "session/x",
				offset: 4,
				created_at: 1,
				verdict: { state: "unverified", fingerprint: "SHA256:y", reason: "no anchor for this key" },
				record: { v: 1, type: "prompt", from: "session/y", to: "session/x", message: "five" },
			}),
		},
		"session/y",
	);
	assert.equal(stranger.answered, true);
	assert.match(stranger.text, /five \(unverified sender\)/);

	// A broken signature is never dressed up as an answer.
	const refused = interpretAsk(
		{
			ok: true,
			text: JSON.stringify({
				topic: "session/x",
				offset: 5,
				created_at: 1,
				verdict: { state: "refused", fingerprint: "SHA256:z", reason: "the signature does not cover this record" },
				record: { v: 1, type: "prompt", from: "session/z", to: "session/x", message: "six" },
			}),
		},
		"session/z",
	);
	assert.equal(refused.answered, false);
	assert.match(refused.text, /failed its signature/);

	const running = interpretAsk({ ok: true, text: '{"outcome":"running","reply_to":7}' }, "session/x");
	assert.equal(running.answered, false);
	assert.match(running.text, /has not answered yet/);

	const finished = interpretAsk(
		{ ok: true, text: '{"outcome":"finished","phase":"completed","reply_to":7}' },
		"session/x",
	);
	assert.equal(finished.answered, false);
	assert.match(finished.text, /finished without answering \(completed\)/);

	// Anything unexpected is reported as it came rather than dressed up as an answer.
	const noise = interpretAsk({ ok: false, text: "could not run msgq: nope" }, "session/x");
	assert.equal(noise.answered, false);
	assert.equal(noise.text, "could not run msgq: nope");
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

test("the plain fallback is the roster without the locator column", () => {
	assert.deepEqual(PLAIN_LIST, ["sessions"]);
});

test("only a CLI that rejected --wide is retried without it", () => {
	const wide = cmd(planPeerAction({ action: "list" }, peers));
	assert.equal(
		shouldRetryPlainList(wide, { ok: false, text: "error: unexpected argument '--wide' found" }),
		true,
	);
	assert.equal(shouldRetryPlainList(wide, { ok: false, text: "could not open the bus" }), false);
	assert.equal(shouldRetryPlainList(wide, { ok: true, text: "" }), false);
});
