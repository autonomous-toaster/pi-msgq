import assert from "node:assert/strict";
import { test } from "node:test";

import { isSealed, openWithClient, parseOpenOutcome } from "../src/open.ts";

const sealed = { v: 1, type: "prompt", from: "session/a", to: "session/b", message: "AAAA", meta: { enc: { alg: "x25519+aes-256-gcm" } } };

test("a plaintext record needs no process", () => {
	const record = { v: 1, type: "note", from: "session/a", to: "session/b", message: "hi" };
	assert.equal(isSealed(record), false);
	let asked = 0;
	const outcome = openWithClient(() => {
		asked += 1;
		return "";
	}, record);
	assert.equal(asked, 0);
	assert.equal(outcome.kind, "opened");
});

test("a sealed record is opened by the client, not here", () => {
	assert.equal(isSealed(sealed), true);
	const outcome = openWithClient(
		(record) => {
			assert.ok(record === sealed);
			return JSON.stringify({ opened: { ...sealed, message: "hi", meta: {} } });
		},
		sealed,
	);
	assert.equal(outcome.kind, "opened");
	if (outcome.kind === "opened") assert.equal(outcome.record.message, "hi");
});

test("somebody else's record is a fact, not a failure", () => {
	const outcome = openWithClient(
		() => JSON.stringify({ sealed_to: ["session/b", "session/c"] }),
		sealed,
	);
	assert.deepEqual(outcome, { kind: "elsewhere", sealedTo: ["session/b", "session/c"] });
});

test("a refusal keeps the reason the client gave", () => {
	assert.deepEqual(parseOpenOutcome(JSON.stringify({ cannot_open: { alg: "future", reason: "not implemented" } })), {
		kind: "cannot",
		alg: "future",
		reason: "not implemented",
	});
	const broken = parseOpenOutcome("not json");
	assert.equal(broken.kind, "cannot");
	assert.match(broken.kind === "cannot" ? broken.reason : "", /not a record/);
});

test("a client that cannot be asked does not swallow the record silently", () => {
	const outcome = openWithClient(() => {
		throw new Error("no such file");
	}, sealed);
	assert.equal(outcome.kind, "cannot");
	assert.match(outcome.kind === "cannot" ? outcome.reason : "", /could not be asked/);
});
