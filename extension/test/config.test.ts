import assert from "node:assert/strict";
import { test } from "node:test";

import {
	canonicalizeAddress,
	ConfigError,
	consumerId,
	parseCardPaths,
	parseContext,
	parsePolicy,
	parsePresence,
	parseTopics,
	parseTtlSeconds,
	parseAllowList,
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
	assert.equal(canonicalizeAddress("build.done"), "session/build.done");
	assert.equal(canonicalizeAddress("session/api-worker"), "session/api-worker");
	assert.equal(canonicalizeAddress("session/me-proj"), "session/me-proj");
	assert.equal(canonicalizeAddress("  session/x  "), "session/x");
	assert.throws(() => canonicalizeAddress("foo/bar"), ConfigError);
	assert.throws(() => canonicalizeAddress(""), ConfigError);
	assert.throws(() => canonicalizeAddress("session/"), ConfigError);
});

test("parseTopics canonicalizes, trims, and de-duplicates", () => {
	assert.deepEqual(parseTopics("session/a, build.done , session/a"), [
		"session/a",
		"session/build.done",
	]);
	assert.deepEqual(parseTopics(undefined), []);
	assert.deepEqual(parseTopics("  "), []);
	assert.throws(() => parseTopics("session/a, foo/bar"), ConfigError);
});

test("parsePolicy defaults to display and rejects unknown values", () => {
	assert.equal(parsePolicy(undefined), "display");
	assert.equal(parsePolicy(""), "display");
	assert.equal(parsePolicy("honor"), "honor");
	assert.equal(parsePolicy("display"), "display");
	assert.throws(() => parsePolicy("trust"), ConfigError);
});

test("a trust list is comma separated, trimmed, and de-duplicated", () => {
	assert.deepEqual(parseAllowList(undefined), []);
	assert.deepEqual(parseAllowList("  "), []);
	assert.deepEqual(parseAllowList("session/planner, cli/ci@host , session/planner"), [
		"session/planner",
		"cli/ci@host",
	]);
});

test("presence is on by default and accepts explicit values", () => {
	assert.equal(parsePresence(undefined), true);
	assert.equal(parsePresence("on"), true);
	assert.equal(parsePresence("OFF"), false);
	assert.equal(parsePresence("0"), false);
	assert.throws(() => parsePresence("maybe"), ConfigError);
});

test("locator context defaults to auto and can be disabled", () => {
	assert.equal(parseContext(undefined), "auto");
	assert.equal(parseContext("AUTO"), "auto");
	assert.equal(parseContext("off"), "off");
	assert.throws(() => parseContext("on"), ConfigError);
});

test("locator path mode defaults to relative and validates", () => {
	assert.equal(parseCardPaths(undefined), "relative");
	assert.equal(parseCardPaths("full"), "full");
	assert.equal(parseCardPaths("none"), "none");
	assert.throws(() => parseCardPaths("short"), ConfigError);
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
		subscriptionAddresses({ alias: "w", sessionId: "abc" }, ["session/a", "session/w"]),
		["session/w", "session/a"],
	);
});

test("a server uri wins over a file path", () => {
	const uri = "postgres://bus@db/msgq";
	assert.equal(resolveDbPath({ MSGQ_DB_URI: uri, MSGQ_DB: "/tmp/x.db" }, "/home/me"), uri);
	assert.equal(resolveDbPath({ MSGQ_DB_URI: " ", MSGQ_DB: "/tmp/x.db" }, "/home/me"), "/tmp/x.db");
	// The older form keeps working: a connection string in MSGQ_DB is a location too.
	assert.equal(resolveDbPath({ MSGQ_DB: uri }, "/home/me"), uri);
	assert.equal(resolveDbPath({}, "/home/me"), "/home/me/.pi/agent/msgq/msgq.db");
});
