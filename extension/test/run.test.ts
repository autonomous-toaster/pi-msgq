import assert from "node:assert/strict";
import { test } from "node:test";

import { runPlanned, summarizeRun } from "../src/run.ts";

test("a run that succeeded reports what it said", () => {
	assert.equal(
		summarizeRun({ command: "msgq", code: 0, stdout: "7\n", binEnv: "MSGQ_BIN" }),
		"7",
	);
});

test("a run that succeeded silently still says something", () => {
	assert.equal(summarizeRun({ command: "msgq", code: 0, stdout: "  ", binEnv: "MSGQ_BIN" }), "(no output)");
});

test("a failure reports the code and what it said", () => {
	assert.equal(
		summarizeRun({ command: "msgq", code: 1, stderr: "no such stream\n", binEnv: "MSGQ_BIN" }),
		"msgq exited with 1: no such stream",
	);
});

test("a failure with stderr prefers stderr over stdout", () => {
	assert.equal(
		summarizeRun({ command: "msgq", code: 2, stdout: "noise", stderr: "the reason", binEnv: "MSGQ_BIN" }),
		"msgq exited with 2: the reason",
	);
});

test("a silent failure names the variable to set", () => {
	const text = summarizeRun({ command: "companion", code: 127, binEnv: "MSGQ_COMPANION" });
	assert.match(text, /was not found/);
	assert.match(text, /MSGQ_COMPANION/);
	assert.match(text, /exited with 127/);
});

test("running a command reports success and the text", async () => {
	const pi = {
		exec: async () => ({ stdout: "session/planner\n", stderr: "", code: 0 }),
	};
	assert.deepEqual(
		await runPlanned(pi, { command: "msgq", args: ["sessions"] }, { binEnv: "MSGQ_BIN" }),
		{ ok: true, text: "session/planner" },
	);
});

test("running a command reports failure, without throwing", async () => {
	const pi = { exec: async () => ({ stdout: "", stderr: "boom", code: 3 }) };
	assert.deepEqual(
		await runPlanned(pi, { command: "msgq", args: ["sessions"] }, { binEnv: "MSGQ_BIN" }),
		{ ok: false, text: "msgq exited with 3: boom" },
	);
});

test("a host that throws is turned into a report, not an exception", async () => {
	const pi = {
		exec: async () => {
			throw new Error("spawn ENOENT");
		},
	};
	const result = await runPlanned(pi, { command: "msgq", args: [] }, { binEnv: "MSGQ_BIN" });
	assert.equal(result.ok, false);
	assert.match(result.text, /could not run msgq/);
	assert.match(result.text, /ENOENT/);
});

test("the timeout and signal are passed through to the host", async () => {
	const seen: Array<{ timeout?: number; signal?: AbortSignal }> = [];
	const pi = {
		exec: async (_command: string, _args: string[], options?: { timeout?: number }) => {
			seen.push(options ?? {});
			return { stdout: "", stderr: "", code: 0 };
		},
	};
	const controller = new AbortController();
	await runPlanned(
		pi,
		{ command: "msgq", args: [] },
		{ binEnv: "MSGQ_BIN", timeoutMs: 1234, signal: controller.signal },
	);
	assert.equal(seen[0].timeout, 1234);
	assert.equal(seen[0].signal, controller.signal);
});
