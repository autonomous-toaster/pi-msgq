import assert from "node:assert/strict";
import { test } from "node:test";

import {
	CARD_MAX_BYTES,
	cardBytes,
	cardSummary,
	cardWithinLimit,
	mergeCard,
	parseCardOverrides,
	parseDerivedCard,
	sameCard,
} from "../src/card.ts";

test("a derived card keeps only known string fields", () => {
	assert.deepEqual(
		parseDerivedCard({
			project: "github.com/org/repo",
			branch: " main ",
			worktree: "repo",
			path: "src",
			head: "abc123",
			description: "not a locator field",
		}),
		{
			project: "github.com/org/repo",
			branch: "main",
			worktree: "repo",
			path: "src",
		},
	);
	assert.deepEqual(parseDerivedCard("nope"), {});
	assert.deepEqual(parseDerivedCard({ project: 7 }), {});
});

test("overrides are validated, trimmed, and cleared with null", () => {
	assert.deepEqual(parseCardOverrides({ project: " org/repo ", branch: " main " }), {
		ok: true,
		overrides: { project: "org/repo", branch: "main" },
	});
	assert.deepEqual(parseCardOverrides({ path: "", worktree: null }), {
		ok: true,
		overrides: { path: null, worktree: null },
	});

	for (const value of [{ head: "abc" }, { project: 3 }, "nope", null]) {
		const parsed = parseCardOverrides(value);
		assert.equal(parsed.ok, false, JSON.stringify(value));
	}
});

test("overrides replace fields and null restores the derived value", () => {
	const base = {
		project: "github.com/org/repo",
		branch: "main",
		worktree: "repo",
		path: "src",
	};
	assert.deepEqual(mergeCard(base, { branch: "review", path: null }), {
		project: "github.com/org/repo",
		branch: "review",
		worktree: "repo",
	});
});

test("the cap is measured on the serialized card", () => {
	const card = { project: "x".repeat(CARD_MAX_BYTES) };
	assert.equal(cardBytes(card) > CARD_MAX_BYTES, true);
	assert.equal(cardWithinLimit(card), false);
	assert.equal(cardWithinLimit({ project: "org/repo" }), true);
});

test("sameCard treats a bare and an absent card alike", () => {
	assert.equal(sameCard(undefined, {}), true);
	assert.equal(sameCard({ project: "x" }, { project: "x" }), true);
	assert.equal(sameCard({ project: "x" }, { project: "y" }), false);
	assert.equal(sameCard(undefined, { project: "x" }), false);
});

test("the summary names project, branch, and a non-root path", () => {
	assert.equal(cardSummary({ project: "org/repo", branch: "main", path: "src" }), "org/repo  main  src");
	assert.equal(cardSummary({ project: "org/repo", branch: "main", path: "." }), "org/repo  main");
	assert.equal(cardSummary(undefined), "");
});
