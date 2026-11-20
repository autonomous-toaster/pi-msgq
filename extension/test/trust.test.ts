import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";

import {
	emptySessionTrust,
	formatTrustReport,
	looksLikeFingerprint,
	parseTrustRequest,
	sessionEnv,
	verdictLine,
	verifyRecord,
} from "../src/trust.ts";

const FP = `SHA256:${"a".repeat(64)}`;
const OTHER = `SHA256:${"b".repeat(64)}`;

test("an empty session layer says nothing at all", () => {
	// An empty variable would be a malformed layer, so absence is the only honest form.
	assert.deepEqual(sessionEnv(emptySessionTrust()), {});
});

test("the session layer is handed to the client under its own name", () => {
	const trust = emptySessionTrust();
	trust.anchors.set("alice", FP);
	trust.anchors.set("bob", OTHER);
	const env = sessionEnv(trust);
	assert.equal(Object.keys(env).length, 1);
	assert.match(env.MSGQ_TRUST_SESSION, /alice:SHA256:a+/);
	assert.match(env.MSGQ_TRUST_SESSION, /bob:SHA256:b+/);
});

test("a verdict reads as one line for each of the three states", () => {
	assert.match(verdictLine({ state: "trusted", principal: "alice", fingerprint: FP }), /^verdict: trusted alice SHA256:aaaaaaaa$/);
	assert.match(verdictLine({ state: "unverified", fingerprint: FP, reason: "no anchor for this key" }), /^verdict: unverified SHA256:aaaaaaaa/);
	assert.match(verdictLine({ state: "refused", fingerprint: FP, reason: "the signature does not cover this record" }), /^verdict: REFUSED .*does not cover/);
	assert.match(verdictLine({ state: "unverified", reason: "no usable signature" }), /^verdict: unverified — no usable signature$/);
});

test("the command has a shape for each verb, and a default", () => {
	assert.deepEqual(parseTrustRequest(""), { action: "list", json: false });
	assert.deepEqual(parseTrustRequest("list --json"), { action: "list", json: true });
	assert.deepEqual(parseTrustRequest(`add alice ${FP}`), {
		action: "add",
		principal: "alice",
		fingerprint: FP,
		session: false,
	});
	assert.deepEqual(parseTrustRequest(`add alice ${FP} --session`), {
		action: "add",
		principal: "alice",
		fingerprint: FP,
		session: true,
	});
	assert.deepEqual(parseTrustRequest("revoke alice"), { action: "revoke", principal: "alice", session: false });
	assert.deepEqual(parseTrustRequest(`rotate alice ${OTHER}`), {
		action: "rotate",
		principal: "alice",
		fingerprint: OTHER,
	});
});

test("a malformed command writes nothing", () => {
	for (const raw of ["add alice", "add alice nonsense", "revoke", "rotate alice", "pin alice"]) {
		const request = parseTrustRequest(raw);
		assert.equal(request.action, "error", raw);
	}
});

test("a fingerprint is a digest or it is not a fingerprint", () => {
	assert.ok(looksLikeFingerprint(FP));
	assert.ok(looksLikeFingerprint("a".repeat(64)));
	assert.ok(looksLikeFingerprint("A".repeat(43)));
	assert.ok(!looksLikeFingerprint(""));
	assert.ok(!looksLikeFingerprint("SHA256:"));
	assert.ok(!looksLikeFingerprint("alice"));
	assert.ok(!looksLikeFingerprint("a".repeat(63)));
});

test("the verdict comes from the client, not from this module", () => {
	const seen: string[] = [];
	const runner = (args: string[], input: string | undefined, env: Record<string, string>) => {
		seen.push(args.join(" "));
		assert.equal(args[0], "verify");
		assert.ok((input ?? "").includes('"from":"session/x"'));
		assert.equal(env.MSGQ_TRUST_SESSION, `alice:${FP}`);
		return JSON.stringify({ state: "trusted", principal: "alice", fingerprint: FP }) + "\n";
	};
	const trust = emptySessionTrust();
	trust.anchors.set("alice", FP);

	const verdict = verifyRecord(runner, { from: "session/x", message: "hi" }, trust);
	assert.equal(verdict.state, "trusted");
	assert.deepEqual(seen, ["verify --json"]);
});

test("a client that cannot answer does not swallow the message", () => {
	const runner = () => {
		throw new Error("no such file");
	};
	const verdict = verifyRecord(runner, { from: "session/x" }, emptySessionTrust());
	assert.equal(verdict.state, "unverified");
	assert.match(verdict.reason ?? "", /could not be asked/);
});

test("the report separates what is durable from what dies with the conversation", () => {
	const trust = emptySessionTrust();
	trust.anchors.set("carol", OTHER);
	trust.unverified = 3;
	const report = formatTrustReport("bus:        local_x\nanchors:    none", trust);
	assert.match(report, /bus:\s+local_x/);
	assert.match(report, /session:\s+\(dies with this conversation\)/);
	assert.match(report, /carol/);
	assert.match(report, /3 record\(s\) came from senders with no anchor/);
});

test("the tool surface cannot reach trust", () => {
	// Trust has to stay an act of the person at the keyboard: a tool would let a record talk the
	// session into believing something about who sent it.
	const source = readFileSync(new URL("../src/tools.ts", import.meta.url), "utf8");
	assert.ok(!source.includes("./trust.ts"), "the tools import trust");
	assert.ok(!source.includes("trust add"), "a tool plans a trust verb");
	assert.ok(!source.includes("registerCommand"), "a tool registers a command");
});

test("a fresh conversation starts with nothing trusted", () => {
	// Two sessions on one machine must not inherit each other's temporary trust.
	const first = emptySessionTrust();
	first.anchors.set("alice", FP);
	const second = emptySessionTrust();
	assert.equal(second.anchors.size, 0);
	assert.deepEqual(sessionEnv(second), {});
});
