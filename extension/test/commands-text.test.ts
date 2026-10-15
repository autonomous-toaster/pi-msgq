import assert from "node:assert/strict";
import { test } from "node:test";

import {
	formatCompanions,
	formatIdentity,
	formatRoster,
	NO_IDENTITY,
	parseCompanionList,
	parseSessions,
	resolveCommandAddress,
	rosterLabel,
	splitPeerArguments,
} from "../src/commands-text.ts";

const SELF = "session/planner";

function address(input: string, self = SELF): string {
	const resolved = resolveCommandAddress(input, self);
	assert.equal(resolved.ok, true, `unexpected refusal: ${JSON.stringify(resolved)}`);
	return resolved.ok ? resolved.address : "";
}

function refusal(input: string, self = SELF): string {
	const resolved = resolveCommandAddress(input, self);
	assert.equal(resolved.ok, false, `unexpected resolution: ${JSON.stringify(resolved)}`);
	return resolved.ok ? "" : resolved.reason;
}

test("me is this session", () => {
	assert.equal(address("me"), SELF);
	assert.equal(address(" me "), SELF);
});

test("me without a bus address is refused", () => {
	assert.equal(refusal("me", ""), NO_IDENTITY);
});

test("a bare name is a session address", () => {
	assert.equal(address("reviewer"), "session/reviewer");
	assert.equal(address("reviewer-2"), "session/reviewer-2");
});

test("an address that carries a scheme is used as given", () => {
	assert.equal(address("session/reviewer"), "session/reviewer");
	assert.equal(address("cli/ci-bot"), "cli/ci-bot");
	assert.equal(address("group/backend"), "group/backend");
});

test("an unknown scheme is refused with the CLI's explanation", () => {
	assert.match(refusal("nope/x"), /unknown address scheme 'nope'/);
});

test("an empty address is refused", () => {
	assert.equal(refusal(""), "no address given");
	assert.equal(refusal("   "), "no address given");
});

test("a scheme with no name is refused", () => {
	assert.match(refusal("session/"), /requires a name/);
});

const LINE = JSON.stringify({
	address: "session/planner",
	alias: "planner",
	expires_at: 1791808616,
	expires_in: 30,
	kind: "session",
	state: "idle",
});

test("the roster parses the bus's json lines", () => {
	assert.deepEqual(parseSessions(LINE), [
		{
			address: "session/planner",
			kind: "session",
			state: "idle",
			alias: "planner",
			expiresIn: 30,
		},
	]);
});

test("several participants parse, in order", () => {
	const other = JSON.stringify({ address: "cli/ci-bot", kind: "cli", state: "idle" });
	const entries = parseSessions([LINE, other].join("\n"));
	assert.deepEqual(
		entries.map((entry) => entry.address),
		["session/planner", "cli/ci-bot"],
	);
});

test("a malformed line is skipped, not fatal", () => {
	const entries = parseSessions(["not json", "", "   ", "[1,2]", LINE].join("\n"));
	assert.deepEqual(
		entries.map((entry) => entry.address),
		["session/planner"],
	);
});

test("an entry without an address is skipped", () => {
	assert.deepEqual(parseSessions(JSON.stringify({ kind: "session" })), []);
});

test("an empty roster is an empty list", () => {
	assert.deepEqual(parseSessions(""), []);
});

test("peer arguments: an address and a message", () => {
	assert.deepEqual(splitPeerArguments("reviewer take a look"), {
		address: "reviewer",
		message: "take a look",
	});
});

test("peer arguments: a quoted message keeps its spaces", () => {
	assert.deepEqual(splitPeerArguments('reviewer "take a look, carefully"'), {
		address: "reviewer",
		message: "take a look, carefully",
	});
});

test("peer arguments: a leading quote means no address", () => {
	assert.deepEqual(splitPeerArguments('"take a look"'), { message: "take a look" });
	assert.deepEqual(splitPeerArguments("'take a look'"), { message: "take a look" });
});

test("peer arguments: an address alone", () => {
	assert.deepEqual(splitPeerArguments("  reviewer  "), { address: "reviewer" });
});

test("peer arguments: nothing at all", () => {
	assert.deepEqual(splitPeerArguments(""), {});
	assert.deepEqual(splitPeerArguments("   "), {});
});

test("a roster row leads with the address", () => {
	assert.equal(rosterLabel({ address: "session/x", state: "idle", expiresIn: 28 }), "session/x  idle  28s");
	assert.equal(rosterLabel({ address: "cli/bot" }), "cli/bot  -");
});

test("the roster lists the other participants, aligned", () => {
	const entries = [
		{ address: "session/me", state: "idle", expiresIn: 30 },
		{ address: "session/reviewer", state: "busy", expiresIn: 28 },
		{ address: "cli/ci", state: "idle" },
	];
	assert.equal(
		formatRoster(entries, "session/me"),
		[
			"  session/reviewer  busy  28s",
			"  cli/ci            idle",
		].join("\n"),
	);
});

test("a roster with nobody else says so", () => {
	assert.equal(
		formatRoster([{ address: "session/me", state: "idle" }], "session/me"),
		"no other participant is reachable on the bus",
	);
	assert.equal(formatRoster([], "session/me"), "no other participant is reachable on the bus");
});

test("the identity report names every fact the session holds", () => {
	const text = formatIdentity({
		self: "session/planner",
		bus: "/home/me/.pi/agent/msgq/msgq.db",
		addresses: ["session/planner", "group/ci"],
		policy: "honor",
		presence: "registered",
		trusted: ["session/parent"],
	});
	assert.equal(
		text,
		[
			"session/planner",
			"  bus         /home/me/.pi/agent/msgq/msgq.db",
			"  follows     session/planner, group/ci",
			"  policy      honor",
			"  presence    registered",
			"  trust       session/parent",
			"  reports to  (none)",
			"  depth       (none: nobody spawned this session)",
		].join("\n"),
	);
});

test("a delegated session is told where it reports and how deep it is", () => {
	const text = formatIdentity({
		self: "session/reviewer-2",
		bus: "db",
		addresses: ["session/reviewer-2"],
		policy: "honor",
		presence: "registered",
		trusted: ["session/planner"],
		reportTo: "session/planner",
		depth: { depth: 1, cap: 2 },
	});
	assert.match(text, /reports to  session\/planner/);
	assert.match(text, /depth       1 of 2/);
});

test("a depth with no cap given is still stated", () => {
	const text = formatIdentity({
		self: "session/x",
		bus: "db",
		addresses: [],
		policy: "display",
		presence: "off",
		trusted: [],
		depth: { depth: 3 },
	});
	assert.match(text, /depth       3$/m);
});

test("an absent depth is stated as absent, not as zero", () => {
	const text = formatIdentity({
		self: "session/x",
		bus: "db",
		addresses: [],
		policy: "display",
		presence: "off",
		trusted: [],
	});
	assert.match(text, /depth       \(none: nobody spawned this session\)/);
	assert.doesNotMatch(text, /depth       0/);
});

test("the identity report distinguishes off from lost", () => {
	const base = {
		self: "session/x",
		bus: "db",
		addresses: ["session/x"],
		policy: "display",
		trusted: [],
	};
	assert.match(formatIdentity({ ...base, presence: "off" }), /presence\s+off \(disabled\)/);
	assert.match(formatIdentity({ ...base, presence: "lost" }), /presence\s+not registered/);
});

test("the identity report admits a session with no bus address", () => {
	const text = formatIdentity({
		self: "",
		bus: "db",
		addresses: [],
		policy: "display",
		presence: "off",
		trusted: [],
	});
	assert.match(text, /\(none: the message bus did not start\)/);
	assert.match(text, /follows\s+\(none\)/);
	assert.match(text, /trust\s+\(none set: every sender\)/);
});

test("companion entries parse from one json object", () => {
	const text = JSON.stringify({
		companions: [
			{ name: "reviewer", address: "session/reviewer", status: "working", cwd: "/src/api" },
			{ name: "helper-2" },
		],
	});
	assert.deepEqual(parseCompanionList(text), [
		{ name: "reviewer", address: "session/reviewer", status: "working", cwd: "/src/api" },
		{ name: "helper-2", address: "session/helper-2", status: undefined, cwd: undefined },
	]);
});

test("an unparseable companion list is empty, not fatal", () => {
	assert.deepEqual(parseCompanionList("nonsense"), []);
	assert.deepEqual(parseCompanionList(""), []);
	assert.deepEqual(parseCompanionList(JSON.stringify({ companions: "nope" })), []);
	assert.deepEqual(parseCompanionList(JSON.stringify({ companions: [null, 3, {}] })), []);
});

test("companions are rendered with their status and directory", () => {
	assert.equal(formatCompanions([]), "no companions are running");
	assert.equal(
		formatCompanions([
			{ name: "reviewer", address: "session/reviewer", status: "working", cwd: "/src/api" },
			{ name: "bot", address: "session/bot" },
		]),
		["  reviewer  working   /src/api", "  bot       -         -"].join("\n"),
	);
});
