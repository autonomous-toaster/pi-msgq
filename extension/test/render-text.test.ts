import assert from "node:assert/strict";
import { test } from "node:test";

import { composeMsgqText, type MsgqTheme, typeColor } from "../src/render-text.ts";

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
		{ items: [{ from: "cli/x", address: "group/a", type: "event", body: "" }] },
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
		{ items: [{ from: "cli/x", address: "group/a", type: "note", body: "line one\nline two" }] },
		plain,
	);
	assert.match(text, /line one\nline two/);
});

test("empty items degrade to the label", () => {
	assert.equal(composeMsgqText({ items: [] }, plain), "msgq");
});

test("type colors are distinct per role", () => {
	assert.equal(typeColor("prompt"), "accent");
	assert.equal(typeColor("steer"), "warning");
	assert.equal(typeColor("note"), "muted");
	assert.equal(typeColor("event"), "dim");
	assert.equal(typeColor("unknown-kind"), "dim");
});
