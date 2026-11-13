// Sealing a body: the rules, and the parity that lets one client open what the other sealed.

import assert from "node:assert/strict";
import { test } from "node:test";
import { readFileSync } from "node:fs";

import {
	describeKey,
	deriveKey,
	kidOf,
	loadKey,
	openBytes,
	sealBytes,
} from "../src/seal.ts";
import { openRecord, sealRecord, type MsgqRecord } from "../src/record.ts";

const PASSPHRASE = "correct horse";

test("the key identifier names the salt, and the CLI derives the same one", () => {
	assert.equal(kidOf("msgq-encryption/v1"), "858ebfb29776c869");
	assert.equal(deriveKey(PASSPHRASE).kid, "858ebfb29776c869");
	assert.notEqual(kidOf("one"), kidOf("two"));
});

test("a sealed body opens with the same key and not with another", () => {
	const key = deriveKey(PASSPHRASE);
	const aad = Buffer.from("msgq/1/note");
	const { nonce, body } = sealBytes(Buffer.from("the plan"), aad, key);
	assert.equal(openBytes(nonce, body, aad, key).toString(), "the plan");
	assert.throws(() => openBytes(nonce, body, aad, deriveKey("battery staple")));
	assert.throws(() => openBytes(nonce, body, Buffer.from("msgq/1/prompt"), key));
});

test("an altered body does not open", () => {
	const key = deriveKey(PASSPHRASE);
	const aad = Buffer.from("msgq/1/note");
	const { nonce, body } = sealBytes(Buffer.from("the plan"), aad, key);
	const sealed = Buffer.from(body, "base64");
	sealed[0] ^= 0x01;
	assert.throws(() => openBytes(nonce, sealed.toString("base64"), aad, key));
});

test("the configured source is read once and says where it came from", () => {
	const key = loadKey({ MSGQ_PASSPHRASE: PASSPHRASE });
	assert.equal(key?.source, "environment");
	const fromFile = loadKey({ MSGQ_PASSPHRASE_FILE: "/tmp/pass" }, () => "correct horse\n");
	assert.equal(fromFile?.source, "file /tmp/pass");
	assert.equal(fromFile?.kid, key?.kid);
	assert.equal(loadKey({}), undefined);
	assert.throws(() => loadKey({ MSGQ_PASSPHRASE_FILE: "/tmp/empty" }, () => "  "));
	assert.match(describeKey(undefined), /plaintext/);
	assert.match(describeKey(key), /aes-256-gcm/);
});

test("a record round-trips, and the declaration is dropped once it is read", () => {
	const key = deriveKey(PASSPHRASE);
	const record: MsgqRecord = { v: 1, type: "note", from: "session/a", to: "session/b", message: "hi" };
	const sealed = sealRecord(record, key);
	assert.equal(typeof sealed.message, "string");
	assert.notEqual(sealed.message, "hi");
	assert.equal((sealed.meta as { enc: { kid: string } }).enc.kid, key.kid);

	const opened = openRecord(sealed, key);
	assert.ok(opened.ok);
	assert.equal(opened.record.message, "hi");
	assert.equal((opened.record.meta as Record<string, unknown>).enc, undefined);
});

test("an event body is sealed too, and comes back as an object", () => {
	const key = deriveKey(PASSPHRASE);
	const record: MsgqRecord = {
		v: 1,
		type: "event",
		from: "session/a",
		to: "session/b",
		event: { kind: "custom", detail: "secret" },
	};
	const sealed = sealRecord(record, key);
	assert.equal(typeof sealed.event, "string");
	const opened = openRecord(sealed, key);
	assert.ok(opened.ok);
	assert.deepEqual(opened.record.event, { kind: "custom", detail: "secret" });
});

test("presence and acknowledgement bodies are never sealed", () => {
	const key = deriveKey(PASSPHRASE);
	for (const kind of ["presence", "ack"]) {
		const record: MsgqRecord = {
			v: 1,
			type: "event",
			from: "session/a",
			to: "session/b",
			event: { kind, state: "idle" },
		};
		assert.deepEqual(sealRecord(record, key).event, { kind, state: "idle" });
	}
});

test("the sealed fixture opens with the test passphrase", () => {
	const raw = readFileSync(new URL("../../protocol/fixtures/16-sealed-body.json", import.meta.url), "utf8");
	const record = JSON.parse(raw) as MsgqRecord;
	const key = deriveKey("correct horse");
	assert.equal((record.meta as { enc: { kid: string } }).enc.kid, key.kid);

	const opened = openRecord(record, key);
	assert.ok(opened.ok);
	assert.equal(opened.record.message, "the plan is tomorrow");

	const none = openRecord(record, undefined);
	assert.equal(none.ok, false);
	assert.match(none.ok === false ? none.reason : "", /no passphrase/);
});

test("a reader without a key refuses, and a reader with the wrong key refuses", () => {
	const sealed = sealRecord(
		{ v: 1, type: "note", from: "session/a", to: "session/b", message: "hi" },
		deriveKey(PASSPHRASE),
	);
	const none = openRecord(sealed, undefined);
	assert.equal(none.ok, false);
	assert.match(none.ok === false ? none.reason : "", /no passphrase/);

	const wrong = openRecord(sealed, deriveKey("battery staple"));
	assert.equal(wrong.ok, false);
	assert.match(wrong.ok === false ? wrong.reason : "", /kid=/);
});
