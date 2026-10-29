// The fixtures in `protocol/fixtures` are the contract two implementations share.
//
// This side checks the reader's tolerance; `cli/src/record.rs` checks the publisher's gate against
// the same file. The vocabulary itself is read out of `PROTOCOL.md`, so the document is the source
// and neither implementation can drift from it unnoticed.

import assert from "node:assert/strict";
import { readFileSync, readdirSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { test } from "node:test";

import { KNOWN_TYPES, decodeRecord } from "../src/record.ts";

const FIXTURES = new URL("../../protocol/fixtures/", import.meta.url);
const PROTOCOL = new URL("../../PROTOCOL.md", import.meta.url);

interface Case {
	file: string;
	publish: "accept" | "reject";
	read: "accept" | "reject";
	mentions?: string;
	why?: string;
}

function expectations(): Case[] {
	const raw = readFileSync(new URL("expectations.json", FIXTURES), "utf8");
	const doc = JSON.parse(raw) as { cases?: Case[] };
	assert.ok(Array.isArray(doc.cases), "expectations.json has no 'cases' array");
	return doc.cases;
}

function fixture(name: string): string {
	return readFileSync(new URL(name, FIXTURES), "utf8");
}

/** The type list the protocol document publishes, which both implementations must agree with. */
function protocolTypes(): string[] {
	const doc = readFileSync(PROTOCOL, "utf8");
	const marker = "The set this version knows:";
	const after = doc.slice(doc.indexOf(marker) + marker.length);
	const block = after.split("```")[1];
	assert.ok(block, "PROTOCOL.md has no fenced block after the known-types marker");
	return block.split(/\s+/).filter((token) => token.length > 0);
}

test("every fixture has an expectation", () => {
	const listed = expectations()
		.map((entry) => entry.file)
		.sort();
	const found = readdirSync(fileURLToPath(FIXTURES))
		.filter((name) => name.endsWith(".json") && name !== "expectations.json")
		.sort();
	assert.deepEqual(found, listed, "every fixture needs an entry in expectations.json");
});

test("the known types are the ones the protocol document lists", () => {
	assert.deepEqual([...KNOWN_TYPES].sort(), protocolTypes().sort());
});

test("fixtures agree with the reader's tolerance", () => {
	for (const entry of expectations()) {
		const payload = JSON.parse(fixture(entry.file));
		const result = decodeRecord(payload);
		if (entry.read === "accept") {
			assert.equal(result.ok, true, `${entry.file}: expected the reader to accept, it refused`);
			continue;
		}
		assert.equal(result.ok, false, `${entry.file}: expected the reader to reject, it accepted`);
		if (entry.mentions) {
			assert.ok(
				!result.ok && result.reason.includes(entry.mentions),
				`${entry.file}: reason does not mention '${entry.mentions}'`,
			);
		}
	}
});

test("what the reader does not know survives decoding", () => {
	for (const entry of expectations()) {
		const payload = JSON.parse(fixture(entry.file)) as Record<string, unknown>;
		const result = decodeRecord(payload);
		if (!result.ok) continue;
		assert.equal(result.record.type, payload.type, `${entry.file}: type was not kept`);
		if (typeof payload.meta === "object" && payload.meta !== null) {
			assert.deepEqual(result.record.meta, payload.meta, `${entry.file}: meta was not kept`);
		}
		if (payload.custom !== undefined) {
			assert.deepEqual(
				(result.record as Record<string, unknown>).custom,
				payload.custom,
				`${entry.file}: an unknown top-level field was dropped`,
			);
		}
	}
});
