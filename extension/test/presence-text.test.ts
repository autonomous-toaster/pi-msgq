import assert from "node:assert/strict";
import { test } from "node:test";

import {
	addressFor,
	aliasFromOwner,
	buildLeaseName,
	parseLeaseName,
} from "../src/presence-text.ts";

test("build and parse round-trip", () => {
	const name = buildLeaseName({ kind: "session", state: "busy", id: "01a127d9" });
	assert.equal(name, "msgq:participant:session:busy:01a127d9");
	assert.deepEqual(parseLeaseName(name), { kind: "session", state: "busy", id: "01a127d9" });
});

test("id is the trailing remainder, so a colon inside it still parses", () => {
	assert.deepEqual(parseLeaseName("msgq:participant:cli:idle:a:b:c"), {
		kind: "cli",
		state: "idle",
		id: "a:b:c",
	});
});

test("malformed names are rejected", () => {
	for (const name of [
		"msgq:participant:session:busy",       // no id
		"msgq:participant:session:01a127d9",   // no state
		"msgq:other:session:busy:x",           // wrong prefix
		"msgq:participant:robot:busy:x",       // unknown kind
		"msgq:participant:session:running:x",  // unknown state
		"msgq:participant:session:busy:",      // empty id
		"nonsense",
	]) {
		assert.equal(parseLeaseName(name), undefined, name);
	}
});

test("alias comes from the owner and is optional", () => {
	assert.equal(aliasFromOwner("api-worker#01a127d9"), "api-worker");
	assert.equal(aliasFromOwner("01a127d9"), undefined);
	assert.equal(aliasFromOwner("#01a127d9"), undefined);
});

test("address prefers the alias, falls back to the id", () => {
	const parts = { kind: "session" as const, state: "idle" as const, id: "01a127d9" };
	assert.equal(addressFor(parts, "api-worker#01a127d9"), "session/api-worker");
	assert.equal(addressFor(parts, "01a127d9"), "session/01a127d9");
});
