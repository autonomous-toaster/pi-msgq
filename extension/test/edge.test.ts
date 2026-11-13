import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";


test("the shared bus holds no direct select", async () => {
	const source = await readFile(new URL("../src/pg-bus.ts", import.meta.url), "utf8");
	const direct = source.match(/FROM\s+pgmq\./gi) ?? [];
	assert.deepEqual(direct, [], "a read must go through the function surface, not a queue table");
});
