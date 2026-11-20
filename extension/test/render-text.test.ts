import assert from "node:assert/strict";
import { test } from "node:test";

import { bodyColor, composeMsgqText, type MsgqTheme, typeColor } from "../src/render-text.ts";

// A theme that returns text unchanged, so assertions read as plain output.
const plain = {
	fg: (_color: string, text: string) => text,
	bold: (text: string) => text,
} as unknown as MsgqTheme;

test("single message shows label, type, route, and body", () => {
	const text = composeMsgqText(
		{ items: [{ from: "cli/alice@mac", address: "session/api-worker", type: "note", body: "build is green" }] },
		plain,
	);
	assert.match(text, /^msgq note$/m);
	assert.match(text, /cli\/alice@mac/);
	assert.match(text, /session\/api-worker/);
	assert.match(text, /build is green/);
});

test("a body-less record still renders a header and route", () => {
	const text = composeMsgqText(
		{ items: [{ from: "cli/x", address: "session/a", type: "event", body: "" }] },
		plain,
	);
	assert.match(text, /^msgq event$/m);
	assert.match(text, /cli\/x/);
	assert.equal(text.endsWith("\n"), false);
});

test("multiple messages are numbered with a count header", () => {
	const text = composeMsgqText(
		{
			items: [
				{ from: "cli/a@h", address: "session/w", type: "prompt", body: "second" },
				{ from: "cli/b@h", address: "session/w", type: "prompt", body: "third" },
			],
		},
		plain,
	);
	assert.match(text, /msgq · 2 messages/);
	assert.match(text, /1\./);
	assert.match(text, /2\./);
	assert.match(text, /cli\/a@h/);
	assert.match(text, /cli\/b@h/);
	assert.match(text, /second/);
	assert.match(text, /third/);
});

test("multi-line bodies keep their lines", () => {
	const text = composeMsgqText(
		{ items: [{ from: "cli/x", address: "session/a", type: "note", body: "line one\nline two" }] },
		plain,
	);
	assert.match(text, /line one\nline two/);
});

test("empty items degrade to the label", () => {
	assert.equal(composeMsgqText({ items: [] }, plain), "msgq");
});

test("a single message says how to answer it", () => {
	const text = composeMsgqText(
		{ items: [{ from: "cli/alice@mac", address: "session/api-worker", type: "note", body: "build is green" }] },
		plain,
	);
	assert.match(text, /reply: \/peer cli\/alice@mac \.\.\./);
});

test("every row of a batch names its own sender", () => {
	const text = composeMsgqText(
		{
			items: [
				{ from: "cli/a@h", address: "session/w", type: "prompt", body: "second" },
				{ from: "cli/b@h", address: "session/w", type: "prompt", body: "third" },
			],
		},
		plain,
	);
	assert.match(text, /reply: \/peer cli\/a@h \.\.\./);
	assert.match(text, /reply: \/peer cli\/b@h \.\.\./);
});

test("a message with no details offers no reply", () => {
	// `?` is the renderer's fallback when a message carries no structured details.
	const text = composeMsgqText(
		{ items: [{ from: "?", address: "?", type: "note", body: "raw text" }] },
		plain,
	);
	assert.match(text, /raw text/);
	assert.doesNotMatch(text, /reply:/);
	assert.doesNotMatch(text, /\/peer/);
});

test("type colors are distinct per role", () => {
	assert.equal(typeColor("prompt"), "accent");
	assert.equal(typeColor("steer"), "warning");
	assert.equal(typeColor("note"), "muted");
	assert.equal(typeColor("event"), "dim");
	assert.equal(typeColor("unknown-kind"), "dim");
});

// A theme that shows which token it was asked for, so a colour can be asserted and not guessed at.
const tinted = {
	fg: (color: string, text: string) => `<${color}>${text}</${color}>`,
	bold: (text: string) => text,
} as unknown as MsgqTheme;

test("a verdict takes its colour from the theme, not from the body", () => {
	const unverified = composeMsgqText(
		{
			items: [
				{
					from: "session/peer",
					address: "session/me",
					type: "prompt",
					body: "hello",
					verdict: { state: "unverified", fingerprint: "SHA256:9fA2c3d4e5f6a7b8", reason: "no anchor for this key" },
				},
			],
		},
		tinted,
	);
	// A sender you have not pinned reads as a warning: the same orange the rest of pi reserves for one.
	// Eight hex digits, the same short form the client prints, so two readers compare one thing.
	assert.match(unverified, /<warning>verdict: unverified SHA256:9fA2c3d4 — no anchor for this key<\/warning>/);
	assert.match(unverified, /no anchor for this key/);
	assert.match(unverified, /<customMessageText>hello</);

	const refused = composeMsgqText(
		{
			items: [
				{
					from: "session/mallory",
					address: "session/me",
					type: "prompt",
					body: "trust me",
					verdict: { state: "refused", fingerprint: "SHA256:aaaa", reason: "the signature does not cover this record" },
				},
			],
		},
		tinted,
	);
	assert.match(refused, /<error>verdict: refused/);

	const trusted = composeMsgqText(
		{
			items: [
				{
					from: "session/alice",
					address: "session/me",
					type: "prompt",
					body: "hello",
					verdict: { state: "trusted", principal: "alice", fingerprint: "SHA256:1a2b3c4d" },
				},
			],
		},
		tinted,
	);
	assert.match(trusted, /<success>verdict: trusted alice SHA256:1a2b3c4d/);
});

test("a record with no verdict says nothing about trust", () => {
	const text = composeMsgqText(
		{ items: [{ from: "cli/x", address: "session/a", type: "note", body: "plain" }] },
		tinted,
	);
	assert.doesNotMatch(text, /verdict:/);
});

test("every message in a batch keeps its own verdict", () => {
	const text = composeMsgqText(
		{
			items: [
				{
					from: "cli/a@h",
					address: "session/w",
					type: "prompt",
					body: "one",
					verdict: { state: "trusted", principal: "a", fingerprint: "SHA256:aaaa" },
				},
				{
					from: "cli/b@h",
					address: "session/w",
					type: "prompt",
					body: "two",
					verdict: { state: "unverified", fingerprint: "SHA256:bbbb" },
				},
			],
		},
		tinted,
	);
	assert.equal((text.match(/verdict:/g) ?? []).length, 2);
	assert.match(text, /<success>verdict: trusted a /);
	assert.match(text, /<warning>verdict: unverified/);
});


test("a record the policy only displayed is dimmed, not dressed as a turn", () => {
	const honored = composeMsgqText(
		{ items: [{ from: "cli/a@h", address: "session/w", type: "prompt", body: "wake up", honored: true }] },
		tinted,
	);
	assert.match(honored, /<customMessageText>wake up</);

	const displayed = composeMsgqText(
		{ items: [{ from: "cli/a@h", address: "session/w", type: "prompt", body: "only shown", honored: false }] },
		tinted,
	);
	assert.match(displayed, /<muted>only shown</);

	// Undecided keeps the message colour: the fallback path has no policy to report.
	assert.equal(bodyColor({ from: "a", address: "b", type: "prompt", body: "", honored: false }), "muted");
	assert.equal(bodyColor({ from: "a", address: "b", type: "prompt", body: "" }), "customMessageText");
});
