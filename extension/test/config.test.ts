import assert from "node:assert/strict";
import { test } from "node:test";

import {
	canonicalizeAddress,
	ConfigError,
	consumerId,
	parsePolicy,
	parsePresence,
	parseTopics,
	parseOptionalAddress,
	parseTtlSeconds,
	parseTrustList,
	resolveDbPath,
	selfAddress,
	subscriptionAddresses,
} from "../src/config.ts";

test("resolveDbPath prefers MSGQ_DB and falls back to the home default", () => {
	assert.equal(resolveDbPath({ MSGQ_DB: "/tmp/x.db" }, "/home/u"), "/tmp/x.db");
	assert.equal(resolveDbPath({ MSGQ_DB: "   " }, "/home/u"), "/home/u/.pi/agent/msgq/msgq.db");
	assert.equal(resolveDbPath({}, "/home/u"), "/home/u/.pi/agent/msgq/msgq.db");
});

test("canonicalizeAddress mirrors the CLI rules", () => {
	assert.equal(canonicalizeAddress("build.done"), "group/build.done");
	assert.equal(canonicalizeAddress("session/api-worker"), "session/api-worker");
	assert.equal(canonicalizeAddress("cwd/Users-me-proj"), "cwd/Users-me-proj");
	assert.equal(canonicalizeAddress("  group/x  "), "group/x");
	assert.throws(() => canonicalizeAddress("foo/bar"), ConfigError);
	assert.throws(() => canonicalizeAddress(""), ConfigError);
	assert.throws(() => canonicalizeAddress("session/"), ConfigError);
});

test("parseTopics canonicalizes, trims, and de-duplicates", () => {
	assert.deepEqual(parseTopics("group/a, build.done , group/a"), [
		"group/a",
		"group/build.done",
	]);
	assert.deepEqual(parseTopics(undefined), []);
	assert.deepEqual(parseTopics("  "), []);
	assert.throws(() => parseTopics("group/a, foo/bar"), ConfigError);
});

test("parsePolicy defaults to display and rejects unknown values", () => {
	assert.equal(parsePolicy(undefined), "display");
	assert.equal(parsePolicy(""), "display");
	assert.equal(parsePolicy("honor"), "honor");
	assert.equal(parsePolicy("display"), "display");
	assert.throws(() => parsePolicy("trust"), ConfigError);
});

test("a trust list is comma separated, trimmed, and de-duplicated", () => {
	assert.deepEqual(parseTrustList(undefined), []);
	assert.deepEqual(parseTrustList("  "), []);
	assert.deepEqual(parseTrustList("session/planner, cli/ci@host , session/planner"), [
		"session/planner",
		"cli/ci@host",
	]);
});

test("an optional address is trimmed or undefined", () => {
	assert.equal(parseOptionalAddress(undefined), undefined);
	assert.equal(parseOptionalAddress("   "), undefined);
	assert.equal(parseOptionalAddress("  session/planner "), "session/planner");
});

test("presence is on by default and accepts explicit values", () => {
	assert.equal(parsePresence(undefined), true);
	assert.equal(parsePresence("on"), true);
	assert.equal(parsePresence("OFF"), false);
	assert.equal(parsePresence("0"), false);
	assert.throws(() => parsePresence("maybe"), ConfigError);
});

test("lease lifetime falls back and validates", () => {
	assert.equal(parseTtlSeconds(undefined, 30), 30);
	assert.equal(parseTtlSeconds(" 5 ", 30), 5);
	assert.throws(() => parseTtlSeconds("0", 30), ConfigError);
	assert.throws(() => parseTtlSeconds("abc", 30), ConfigError);
});

test("selfAddress uses the alias when present, else the session id", () => {
	assert.equal(selfAddress({ alias: "api-worker", sessionId: "abc" }), "session/api-worker");
	assert.equal(selfAddress({ alias: "", sessionId: "abc" }), "session/abc");
	assert.equal(selfAddress({ sessionId: "abc" }), "session/abc");
});

test("consumerId is unique even when two sessions share an alias", () => {
	assert.equal(consumerId({ alias: "worker", sessionId: "abc" }), "worker#abc");
	assert.equal(consumerId({ sessionId: "abc" }), "abc");
	assert.notEqual(
		consumerId({ alias: "worker", sessionId: "one" }),
		consumerId({ alias: "worker", sessionId: "two" }),
	);
});

test("subscriptionAddresses always includes self and de-duplicates extras", () => {
	assert.deepEqual(
		subscriptionAddresses({ alias: "w", sessionId: "abc" }, ["group/a", "session/w"]),
		["session/w", "group/a"],
	);
});
