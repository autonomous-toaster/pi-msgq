// End-to-end driver for the msgq extension.
//
// Runs pi in RPC mode with `-e <this extension>`, publishes with the msgq CLI,
// and inspects what the extension delivered into the session.
//
//   node scripts/e2e.mjs <default-note|honor-prompt|coalesce|note-busy|bad-bus|skip-version|...>
//
// Scenarios whose name starts with `command-` drive the extension's slash commands,
// `companion-list` drives the companion command with a fake herdr, and the rest cover
// delivery, presence and acknowledgements. Run it with no argument for the full list.
//
// Environment overrides: MSGQ_BIN (path to the msgq binary), PI_BIN (pi command),
// MSGQ_DB (bus location).
import { execFileSync, spawn, spawnSync } from "node:child_process";
import { chmodSync, mkdtempSync, writeFileSync } from "node:fs";
import { createRequire } from "node:module";
import { tmpdir } from "node:os";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const require_ = createRequire(import.meta.url);
const honker = require_("@russellthehippo/honker-node");

const HERE = dirname(fileURLToPath(import.meta.url));
const EXTENSION = process.env.MSGQ_EXTENSION ?? resolve(HERE, "..");
/** The address every participant publishes presence to. */
const PRESENCE_ADDRESS = "group/presence";

const REPO = resolve(EXTENSION, "..");
const CLI = process.env.MSGQ_BIN ?? resolve(REPO, "cli/target/debug/msgq");
const PI = process.env.PI_BIN ?? "pi";
const DB =
	process.env.MSGQ_DB ?? resolve(mkdtempSync(resolve(tmpdir(), "msgq-e2e-")), "msgq.db");

const wait = (ms) => new Promise((r) => setTimeout(r, ms));

function publish(address, message, ...extra) {
	return execFileSync(CLI, ["--db", DB, "publish", address, message, ...extra], {
		encoding: "utf8",
	}).trim();
}

/** Publish straight to the engine, bypassing the CLI's record validation. */
function publishRaw(dbPath, address, payload) {
	const db = honker.open(dbPath, { watcherPollIntervalMs: 50 });
	const offset = db.stream(address).publish(payload);
	db.close();
	return offset;
}

function startPi(extraArgs = [], envOverrides = {}, { noTools = true } = {}) {
	const toolArgs = noTools ? ["-nt"] : [];
	const child = spawn(PI, ["--mode", "rpc", "-e", EXTENSION, ...toolArgs, ...extraArgs], {
		cwd: REPO,
		env: { ...process.env, MSGQ_DB: DB, ...envOverrides },
		stdio: ["pipe", "pipe", "ignore"],
	});

	let buffer = "";
	const pending = new Map();
	const events = [];
	const ui = [];

	child.stdout.on("data", (chunk) => {
		buffer += chunk.toString();
		let index;
		while ((index = buffer.indexOf("\n")) >= 0) {
			const line = buffer.slice(0, index).replace(/\r$/, "");
			buffer = buffer.slice(index + 1);
			if (!line) continue;
			let record;
			try {
				record = JSON.parse(line);
			} catch {
				continue;
			}
			if (record.type === "response" && record.id && pending.has(record.id)) {
				pending.get(record.id)(record);
				pending.delete(record.id);
			} else if (record.type === "extension_ui_request") {
				ui.push(record);
			} else if (record.type) {
				events.push(record.type);
			}
		}
	});

	const send = (command) =>
		new Promise((resolve_) => {
			pending.set(command.id, resolve_);
			child.stdin.write(`${JSON.stringify(command)}\n`);
		});

	const waitForEvent = async (name, timeoutMs) => {
		const deadline = Date.now() + timeoutMs;
		while (Date.now() < deadline) {
			if (events.includes(name)) return true;
			await wait(50);
		}
		return false;
	};

	return { child, send, events, ui, waitForEvent };
}

/** The stored consumer cursor for (consumer, topic), or -1 when absent. */
function cursorFor(db, consumer, topic) {
	const handle = honker.open(db, { watcherPollIntervalMs: 50 });
	const rows = handle.query(
		"SELECT offset FROM _honker_stream_consumers WHERE name = ? AND topic = ?",
		[consumer, topic],
	);
	handle.close();
	return rows.length > 0 ? Number(rows[0].offset) : -1;
}

function shutdown(child) {
	try {
		child.stdin.end();
	} catch {}
	setTimeout(() => child.kill(), 300);
}

/** A stream's records, or [] when nothing was ever published to it. */
function readStream(address, db = DB) {
	const out = execFileSync(CLI, ["--db", db, "read", address, "--from-start", "--json"], {
		encoding: "utf8",
	}).trim();
	return out ? out.split("\n").map((line) => JSON.parse(line)) : [];
}

/**
 * Take a presence lease by hand, so a scenario has a peer to talk to without
 * starting a second pi. The address is derived from the owner's alias.
 */
/**
 * Announce a participant the way any client would: one presence record, published with nothing
 * but the CLI. Presence is a record now, so a scenario does not need the engine to fake a peer.
 */
function announcePeer(dbPath, alias, id, ttlS = 60) {
	const record = {
		v: 1,
		type: "event",
		from: `session/${alias ?? id}`,
		to: "group/presence",
		event: { kind: "presence", state: "idle", ttl: ttlS, participant: id, alias },
	};
	try {
		execFileSync(CLI, ["--db", dbPath, "publish", "group/presence", "--json", JSON.stringify(record)], {
			stdio: "ignore",
		});
		return true;
	} catch {
		return false;
	}
}

/** Run a slash command in a session and report what the prompt did with it. */
async function runCommand(session, text, settleMs = 1500) {
	const response = await session.send({ id: `cmd-${Date.now()}`, type: "prompt", message: text });
	await wait(settleMs);
	return response.data ?? {};
}

/** Everything a session said to the human since `from`, as plain strings. */
function notices(session, from = 0) {
	return session.ui
		.slice(from)
		.filter((record) => record.method === "notify")
		.map((record) => String(record.message ?? ""));
}

/** Answer the next dialog of this kind. The payload may be a function of the request. */
async function answerDialog(session, method, payload) {
	const answered = new Set(session.ui.filter((r) => r.__answered).map((r) => r.id));
	for (let i = 0; i < 120; i++) {
		const request = session.ui.find((r) => r.method === method && !answered.has(r.id));
		if (request) {
			request.__answered = true;
			const body = typeof payload === "function" ? payload(request) : payload;
			session.child.stdin.write(
				`${JSON.stringify({ type: "extension_ui_response", id: request.id, ...body })}\n`,
			);
			return request;
		}
		await wait(50);
	}
	return undefined;
}

/** Run pi once, with no session to talk to: the no-UI path. */
function oneShot(command, env = {}, db = DB) {
	const result = spawnSync(PI, ["--mode", "json", "-p", command, "-e", EXTENSION], {
		cwd: REPO,
		env: { ...process.env, MSGQ_DB: db, ...env },
		encoding: "utf8",
	});
	return { out: result.stdout ?? "", err: result.stderr ?? "", code: result.status ?? -1 };
}

function split(entries) {
	return {
		custom: entries.filter((e) => e.type === "custom_message"),
		assistants: entries.filter((e) => e.type === "message" && e.message?.role === "assistant"),
	};
}

function report(label, ok, detail) {
	console.log(`${detail} ${ok ? "RESULT: PASS" : "RESULT: FAIL"} ${label}`);
	process.exit(ok ? 0 : 1);
}

async function scenarioDefaultNote() {
	const { child, send, ui } = startPi();
	const state = await send({ id: "s", type: "get_state" });
	const sessionId = state.data.sessionId;
	console.log("session:", sessionId, "policy: default");

	publish(`session/${sessionId}`, "hello from cli");
	await wait(2500);

	const { data } = await send({ id: "e", type: "get_entries" });
	const { custom, assistants } = split(data.entries);
	shutdown(child);

	const notified = ui.some((r) => JSON.stringify(r).includes("msgq"));
	const ok =
		custom.length === 1 &&
		custom[0].customType === "msgq" &&
		String(custom[0].content).includes("hello from cli") &&
		String(custom[0].content).includes(`session/${sessionId}`) &&
		assistants.length === 0 &&
		notified;
	report(
		"default-note",
		ok,
		`custom=${custom.length} assistants=${assistants.length} notified=${notified}`,
	);
}

async function scenarioHonorPrompt() {
	const { child, send, waitForEvent } = startPi(["--msgq-policy", "honor"]);
	const state = await send({ id: "s", type: "get_state" });
	const sessionId = state.data.sessionId;
	console.log("session:", sessionId, "policy: honor");

	publish(`session/${sessionId}`, "Reply with exactly: pong");
	await waitForEvent("agent_settled", 90_000);
	await wait(1000);

	const { data } = await send({ id: "e", type: "get_entries" });
	const { custom: allCustom, assistants } = split(data.entries);
	// Other extensions may add entries of their own, so count the ones this one owns.
	const custom = allCustom.filter((entry) => entry.customType === "msgq");
	shutdown(child);

	report(
		"honor-prompt",
		custom.length === 1 && assistants.length >= 1,
		`custom=${custom.length} assistants=${assistants.length}`,
	);
}

async function scenarioCoalesce() {
	const { child, send, waitForEvent } = startPi(["--msgq-policy", "honor"]);
	const state = await send({ id: "s", type: "get_state" });
	const sessionId = state.data.sessionId;
	console.log("session:", sessionId, "policy: honor (coalesce)");

	const address = `session/${sessionId}`;
	publish(address, "Think quietly, then reply with: first");
	await waitForEvent("agent_start", 60_000);
	publish(address, "second");
	publish(address, "third");

	await waitForEvent("agent_settled", 90_000);
	await wait(1500);

	const { data } = await send({ id: "e", type: "get_entries" });
	const { custom, assistants } = split(data.entries);
	const batches = custom.filter((e) => String(e.content).includes("messages"));
	shutdown(child);

	report(
		"coalesce",
		batches.length === 1 && assistants.length === 2,
		`batches=${batches.length} assistants=${assistants.length}`,
	);
}

async function scenarioNoteBusy() {
	const { child, send, waitForEvent } = startPi(["--msgq-policy", "honor"]);
	const state = await send({ id: "s", type: "get_state" });
	const sessionId = state.data.sessionId;
	console.log("session:", sessionId, "policy: honor (note while busy)");

	const address = `session/${sessionId}`;
	publish(address, "Reply with exactly: pong");
	await waitForEvent("agent_start", 60_000);
	publish(address, "a note mid-run", "--type", "note");

	await waitForEvent("agent_settled", 90_000);
	await wait(1500);

	const { data } = await send({ id: "e", type: "get_entries" });
	const { custom, assistants } = split(data.entries);
	const note = custom.find((e) => String(e.content).includes("a note mid-run"));
	shutdown(child);


	report(
		"note-busy",
		Boolean(note) && assistants.length === 1,
		`note=${Boolean(note)} assistants=${assistants.length}`,
	);
}

async function scenarioBadBus() {
	const dir = mkdtempSync(resolve(tmpdir(), "msgq-badbus-"));
	const blocker = resolve(dir, "not-a-dir");
	writeFileSync(blocker, "x");
	const badPath = resolve(blocker, "msgq.db");

	const { child, send, ui } = startPi([], { MSGQ_DB: badPath });
	const state = await send({ id: "s", type: "get_state" });
	const alive = state.success === true;
	await wait(1000);
	shutdown(child);

	const disabled = ui.some((r) => JSON.stringify(r).includes("disabled"));
	report("bad-bus", alive && disabled, `alive=${alive} disabled=${disabled}`);
}

async function scenarioSkipVersion() {
	const { child, send } = startPi();
	const state = await send({ id: "s", type: "get_state" });
	const sessionId = state.data.sessionId;
	console.log("session:", sessionId, "policy: default (skip version)");

	const address = `session/${sessionId}`;
	publishRaw(DB, address, {
		v: 2,
		type: "note",
		from: "cli/x",
		to: address,
		message: "from the future",
	});
	await wait(2500);

	const { data } = await send({ id: "e", type: "get_entries" });
	const { custom } = split(data.entries);
	const replies = execFileSync(CLI, ["--db", DB, "read", "cli/x", "--from-start", "--json"], {
		encoding: "utf8",
	}).trim();
	shutdown(child);

	report(
		"skip-version",
		custom.length === 0 && replies === "",
		`custom=${custom.length} replies=${JSON.stringify(replies)}`,
	);
}

async function scenarioNoteIdle() {
	const { child, send } = startPi(["--msgq-policy", "honor"]);
	const state = await send({ id: "s", type: "get_state" });
	const sessionId = state.data.sessionId;
	console.log("session:", sessionId, "policy: honor (note while idle)");

	publish(`session/${sessionId}`, "a note while idle", "--type", "note");
	await wait(2500);

	const { data } = await send({ id: "e", type: "get_entries" });
	const { custom, assistants } = split(data.entries);
	shutdown(child);

	report(
		"note-idle",
		custom.length === 1 && assistants.length === 0,
		`custom=${custom.length} assistants=${assistants.length}`,
	);
}

async function scenarioSelfGuard() {
	const { child, send } = startPi(["--msgq-policy", "honor"]);
	const state = await send({ id: "s", type: "get_state" });
	const sessionId = state.data.sessionId;
	const address = `session/${sessionId}`;
	console.log("session:", sessionId, "policy: honor (self-addressed)");

	publish(address, "a self-addressed prompt", "--from", address);
	await wait(2500);

	const { data } = await send({ id: "e", type: "get_entries" });
	const { custom, assistants } = split(data.entries);
	shutdown(child);

	report(
		"self-guard",
		custom.length === 1 && assistants.length === 0,
		`custom=${custom.length} assistants=${assistants.length}`,
	);
}

async function scenarioSteerMidRun() {
	const { child, send, waitForEvent } = startPi(["--msgq-policy", "honor"]);
	const state = await send({ id: "s", type: "get_state" });
	const sessionId = state.data.sessionId;
	const address = `session/${sessionId}`;
	console.log("session:", sessionId, "policy: honor (steer mid-run)");

	publish(address, "Think quietly, then reply with: first");
	await waitForEvent("agent_start", 60_000);
	publish(address, "change of plan", "--type", "steer");

	await waitForEvent("agent_settled", 90_000);
	await wait(1500);

	const { data } = await send({ id: "e", type: "get_entries" });
	const { custom, assistants } = split(data.entries);
	const steer = custom.find((e) => String(e.content).includes("change of plan"));
	shutdown(child);

	report(
		"steer-mid-run",
		Boolean(steer) && assistants.length === 2,
		`steer=${Boolean(steer)} assistants=${assistants.length}`,
	);
}

async function scenarioExtraTopic() {
	const { child, send } = startPi(["--msgq-topics", "group/ci"]);
	const state = await send({ id: "s", type: "get_state" });
	const sessionId = state.data.sessionId;
	console.log("session:", sessionId, "extra topic: group/ci");

	publish("group/ci", "build is green", "--type", "note");
	await wait(2500);

	const { data } = await send({ id: "e", type: "get_entries" });
	const { custom } = split(data.entries);
	const got = custom.some((e) => String(e.content).includes("build is green"));
	shutdown(child);

	report("extra-topic", got && custom.length === 1, `custom=${custom.length} got=${got}`);
}


async function scenarioPresence() {
	const { child, send, waitForEvent } = startPi(["--msgq-policy", "honor"]);
	const state = await send({ id: "s", type: "get_state" });
	const sessionId = state.data.sessionId;
	const address = `session/${sessionId}`;
	console.log("session:", sessionId, "policy: honor (presence)");

	const entries = () =>
		execFileSync(CLI, ["--db", DB, "sessions", "--json"], { encoding: "utf8" })
			.trim()
			.split("\n")
			.filter(Boolean)
			.map((line) => JSON.parse(line));
	const find = (list) => list.find((entry) => entry.address === address);
	const observe = async (want) => {
		let seen = find(entries());
		for (let i = 0; i < 50 && seen?.state !== want; i++) {
			await wait(100);
			seen = find(entries());
		}
		return seen;
	};

	const online = await observe("idle");
	console.log("registered:", JSON.stringify(online));

	publish(address, "Reply with exactly: pong");
	await waitForEvent("agent_start", 60_000);
	const busy = await observe("busy");
	console.log("during run:", busy?.state);

	await waitForEvent("agent_settled", 90_000);
	const idle = await observe("idle");
	console.log("after run:", idle?.state);

	shutdown(child);
	const ok = Boolean(online) && busy?.state === "busy" && idle?.state === "idle";
	report("presence", ok, `online=${Boolean(online)} busy=${busy?.state} idle=${idle?.state}`);
}


async function scenarioScopedWrite() {
	// A dedicated bus, so "no streams" means the extension wrote none, not that
	// some earlier scenario had not published.
	const own = resolve(mkdtempSync(resolve(tmpdir(), "msgq-scoped-")), "msgq.db");
	const { child, send } = startPi([], { MSGQ_DB: own });
	await send({ id: "s", type: "get_state" });

	let online = false;
	for (let i = 0; i < 40; i++) {
		if (execFileSync(CLI, ["--db", own, "sessions", "--json"], { encoding: "utf8" }).trim()) {
			online = true;
			break;
		}
		await wait(100);
	}

	const native = honker.open(own, { watcherPollIntervalMs: 50 });
	const topics = native.query("SELECT DISTINCT topic FROM _honker_stream").map((row) => String(row.topic));
	const streams = topics.length;
	const leases = Number(
		native.query("SELECT COUNT(*) AS n FROM _honker_locks WHERE name LIKE 'msgq:participant:%'")[0].n,
	);
	native.close();
	shutdown(child);

	// Presence is a record now, so the one stream the extension appends to is the presence
	// address — and nothing else. The engine's lock table is no longer part of presence at all.
	const ok = online && streams === 1 && topics.length === 1 && topics[0] === PRESENCE_ADDRESS && leases === 0;
	report(
		"scoped-write",
		ok,
		`online=${online} streams=${streams} topics=${topics.join(",") || "-"} participantLeases=${leases}`,
	);
}

async function scenarioCrashExpiry() {
	const { child, send } = startPi(["--msgq-policy", "honor"], { MSGQ_PRESENCE_TTL: "2" });
	const state = await send({ id: "s", type: "get_state" });
	const address = `session/${state.data.sessionId}`;
	const listed = () =>
		execFileSync(CLI, ["--db", DB, "sessions", "--json"], { encoding: "utf8" }).includes(address);

	let online = false;
	for (let i = 0; i < 40; i++) {
		if (listed()) {
			online = true;
			break;
		}
		await wait(100);
	}

	// Hard kill: no shutdown handler runs, so only the TTL can clear the lease.
	child.kill("SIGKILL");

	let agedOut = false;
	for (let i = 0; i < 75; i++) {
		await wait(200);
		if (!listed()) {
			agedOut = true;
			break;
		}
	}
	report("crash-expiry", online && agedOut, `online=${online} agedOut=${agedOut}`);
}


async function scenarioRename() {
	const own = resolve(mkdtempSync(resolve(tmpdir(), "msgq-rename-")), "msgq.db");
	const id = "00000000-0000-7000-8000-000000000001";
	const { child, send } = startPi(["--session-id", id], { MSGQ_DB: own });
	await send({ id: "s", type: "get_state" });

	const listed = () =>
		execFileSync(CLI, ["--db", own, "sessions", "--json"], { encoding: "utf8" })
			.trim()
			.split("\n")
			.filter(Boolean)
			.map((line) => JSON.parse(line));

	let before;
	for (let i = 0; i < 40; i++) {
		const list = listed();
		if (list.length > 0) {
			before = list[0];
			break;
		}
		await wait(100);
	}
	console.log("before rename:", before?.address);

	const resp = await send({ id: "n", type: "set_session_name", name: "api-worker" });
	console.log("rename accepted:", resp.success);

	let after;
	for (let i = 0; i < 50; i++) {
		const list = listed();
		if (list[0]?.address === "session/api-worker") {
			after = list[0];
			break;
		}
		await wait(100);
	}
	console.log("after rename:", after?.address);

	// The subscription has to follow the new address, or the session stops receiving at
	// the very name the roster now advertises.
	execFileSync(CLI, ["--db", own, "publish", "session/api-worker", "ping after rename", "--type", "note"]);
	await wait(2500);
	const { data } = await send({ id: "e", type: "get_entries" });
	const received = data.entries.some(
		(entry) => entry.type === "custom_message" && String(entry.content).includes("ping after rename"),
	);
	console.log("delivered at the new address:", received);

	shutdown(child);
	const ok =
		before?.address === `session/${id}` && after?.alias === "api-worker" && received;
	report(
		"rename",
		ok,
		`before=${before?.address} after=${after?.address} received=${received}`,
	);
}

async function scenarioSquat() {
	// A dedicated bus and a pinned session id. A foreign participant announces presence for the
	// same address before the session exists. Under records there is no lease to win, so what has
	// to hold is that the foreign record survives, the session appears beside it, consumption is
	// unaffected, and the collision is said out loud instead of being silently doubled.
	const own = resolve(mkdtempSync(resolve(tmpdir(), "msgq-squat-")), "msgq.db");
	const id = "00000000-0000-7000-8000-000000000002";
	const address = `session/${id}`;

	const squatter = {
		v: 1,
		type: "event",
		from: address,
		to: PRESENCE_ADDRESS,
		event: { kind: "presence", state: "idle", ttl: 120, participant: "squatter", alias: "squatter" },
	};
	execFileSync(CLI, ["--db", own, "publish", PRESENCE_ADDRESS, "--json", JSON.stringify(squatter)]);

	const session = startPi(["--session-id", id, "--msgq-policy", "honor"], { MSGQ_DB: own });
	const { child, send } = session;
	await send({ id: "s", type: "get_state" });

	// Consumption must continue even though another participant claims this address.
	execFileSync(CLI, ["--db", own, "publish", address, "squat delivery check"]);
	await wait(2500);

	const { data } = await send({ id: "e", type: "get_entries" });
	const delivered = data.entries.some(
		(entry) => entry.type === "custom_message" && String(entry.content).includes("squat delivery check"),
	);

	const roster = execFileSync(CLI, ["--db", own, "sessions", "--json"], { encoding: "utf8" })
		.trim()
		.split("\n")
		.filter(Boolean)
		.map((line) => JSON.parse(line));
	const atAddress = roster.filter((entry) => entry.address === address);
	const warned = notices(session).some((line) => line.includes("publishing presence at"));

	shutdown(child);
	const ok = delivered && atAddress.length === 2 && warned;
	report(
		"squat",
		ok,
		`delivered=${delivered} entriesAtAddress=${atAddress.length} warned=${warned}`,
	);
}

async function scenarioPresenceOff() {
	const own = resolve(mkdtempSync(resolve(tmpdir(), "msgq-off-")), "msgq.db");
	const { child, send } = startPi([], { MSGQ_DB: own, MSGQ_PRESENCE: "off" });
	const state = await send({ id: "s", type: "get_state" });
	const address = `session/${state.data.sessionId}`;

	// Consumption must be unaffected by the opt-out.
	execFileSync(CLI, ["--db", own, "publish", address, "off but consuming"]);
	await wait(2500);
	const { data } = await send({ id: "e", type: "get_entries" });
	const delivered = data.entries.some(
		(entry) => entry.type === "custom_message" && String(entry.content).includes("off but consuming"),
	);
	const listed = execFileSync(CLI, ["--db", own, "sessions", "--json"], { encoding: "utf8" }).trim();
	shutdown(child);

	report("presence-off", delivered && listed === "", `delivered=${delivered} listed=${JSON.stringify(listed)}`);
}

async function scenarioBlocked() {
	const own = resolve(mkdtempSync(resolve(tmpdir(), "msgq-blocked-")), "msgq.db");
	const probe = resolve(HERE, "probe-prompt.ts");
	const { child, send, ui } = startPi(["-e", probe], { MSGQ_DB: own });
	const state = await send({ id: "s", type: "get_state" });
	const address = `session/${state.data.sessionId}`;

	// Open the blocking prompt from a command. Do not await the response: the
	// command handler blocks on the prompt, so its response only lands after we
	// answer it.
	child.stdin.write(`${JSON.stringify({ id: "p", type: "prompt", message: "/blockme" })}\n`);

	const listed = () =>
		execFileSync(CLI, ["--db", own, "sessions", "--json"], { encoding: "utf8" })
			.trim()
			.split("\n")
			.filter(Boolean)
			.map((line) => JSON.parse(line))
			.find((entry) => entry.address === address);

	let blocked;
	for (let i = 0; i < 50; i++) {
		const entry = listed();
		if (entry?.state === "blocked") {
			blocked = entry;
			break;
		}
		await wait(100);
	}
	console.log("while blocked:", blocked?.state);

	// Answer the prompt so it closes, then expect idle again.
	// The request may land a beat after the state flips, so wait for it.
	let request;
	for (let i = 0; i < 50; i++) {
		request = ui.find((record) => record.method === "confirm");
		if (request) break;
		await wait(100);
	}
	if (request) {
		child.stdin.write(`${JSON.stringify({ type: "extension_ui_response", id: request.id, confirmed: false })}\n`);
	}

	let idle;
	for (let i = 0; i < 50; i++) {
		const entry = listed();
		if (entry?.state === "idle") {
			idle = entry;
			break;
		}
		await wait(100);
	}
	console.log("after prompt closed:", idle?.state);

	shutdown(child);
	report("blocked", blocked?.state === "blocked" && idle?.state === "idle", `blocked=${blocked?.state} after=${idle?.state}`);
}


async function scenarioAckDisplay() {
	const own = resolve(mkdtempSync(resolve(tmpdir(), "msgq-ackdisp-")), "msgq.db");
	const { child, send } = startPi(["--session-id", "00000000-0000-7000-8000-000000000003"], {
		MSGQ_DB: own,
	});
	await send({ id: "s", type: "get_state" });
	const address = "session/00000000-0000-7000-8000-000000000003";

	const record = JSON.stringify({
		v: 1,
		type: "note",
		from: "cli/e2e@test",
		to: address,
		message: "ack me",
		meta: { ack: true },
	});

	let out = "";
	let code = 0;
	try {
		out = execFileSync(
			CLI,
			["--db", own, "publish", address, "--json", record, "--await", "--timeout", "20"],
			{ encoding: "utf8" },
		);
	} catch (error) {
		code = error.status ?? -1;
		out = String(error.stdout ?? "");
	}
	console.log("await output:", JSON.stringify(out.trim()), "exit:", code);
	const published = Number(out.split("\n")[0]);
	const cursor = cursorFor(own, "00000000-0000-7000-8000-000000000003", address);
	shutdown(child);

	const ok =
		out.includes("received") &&
		out.includes("delivery=display") &&
		code === 0 &&
		cursor === published;
	report("ack-display", ok, `exit=${code} persisted=${cursor} published=${published}`);
}


async function scenarioAckHonor() {
	const own = resolve(mkdtempSync(resolve(tmpdir(), "msgq-ackhonor-")), "msgq.db");
	const id = "00000000-0000-7000-8000-000000000004";
	const { child, send } = startPi(["--session-id", id, "--msgq-policy", "honor"], { MSGQ_DB: own });
	await send({ id: "s", type: "get_state" });
	const address = `session/${id}`;

	const record = JSON.stringify({
		v: 1, type: "prompt", from: "cli/e2e@test", to: address,
		message: "Reply with exactly: pong", meta: { ack: true },
	});
	let out = "";
	let code = 0;
	try {
		out = execFileSync(CLI, ["--db", own, "publish", address, "--json", record, "--await", "--timeout", "90"], { encoding: "utf8" });
	} catch (error) {
		code = error.status ?? -1;
		out = String(error.stdout ?? "");
	}
	console.log("await output:", JSON.stringify(out.trim()));
	shutdown(child);
	const ok = ["delivery=turn", "started", "completed", "outcome=ok"].every((s) => out.includes(s)) && code === 0;
	report("ack-honor", ok, `exit=${code} out=${JSON.stringify(out.trim())}`);
}

async function scenarioAckBatch() {
	const own = resolve(mkdtempSync(resolve(tmpdir(), "msgq-ackbatch-")), "msgq.db");
	const id = "00000000-0000-7000-8000-000000000005";
	const { child, send, waitForEvent } = startPi(["--session-id", id, "--msgq-policy", "honor"], { MSGQ_DB: own });
	await send({ id: "s", type: "get_state" });
	const address = `session/${id}`;

	// One record starts a run; two acknowledged records arrive while it is busy.
	execFileSync(CLI, ["--db", own, "publish", address, "Think quietly, then reply with: first"]);
	await waitForEvent("agent_start", 60_000);

	const mk = (text) => JSON.stringify({ v: 1, type: "prompt", from: "cli/e2e@test", to: address, message: text, meta: { ack: true } });
	const offB = Number(execFileSync(CLI, ["--db", own, "publish", address, "--json", mk("second")], { encoding: "utf8" }).trim());
	const offC = Number(execFileSync(CLI, ["--db", own, "publish", address, "--json", mk("third")], { encoding: "utf8" }).trim());
	console.log("buffered offsets:", offB, offC);

	const awaitOne = (offset) => {
		try {
			return execFileSync(CLI, ["--db", own, "await", String(offset), "--on", "cli/e2e@test", "--timeout", "60"], { encoding: "utf8" });
		} catch (error) {
			return String(error.stdout ?? "");
		}
	};

	// One run settles, the batch is injected, and each record is acknowledged on its own.
	await waitForEvent("agent_settled", 90_000);
	const outB = awaitOne(offB);
	const outC = awaitOne(offC);
	console.log("B:", JSON.stringify(outB.trim()));
	console.log("C:", JSON.stringify(outC.trim()));
	shutdown(child);

	const cursor = cursorFor(own, id, address);
	console.log("persisted cursor:", cursor, "last buffered:", offC);
	for (const out of [outB, outC]) {
		if (!out.includes("delivery=queued") || !out.includes("started") || !out.includes("completed")) {
			report("ack-batch", false, `missing phase in ${JSON.stringify(out.trim())}`);
		}
	}
	report("ack-batch", cursor >= offC, `cursor=${cursor} offC=${offC}`);
}

async function scenarioAckQuiet() {
	const own = resolve(mkdtempSync(resolve(tmpdir(), "msgq-quiet-")), "msgq.db");
	// Unique per run: a pinned id would resume the previous run's session file.
	const id = `00000000-0000-7000-8000-${String(Date.now()).padStart(12, "0").slice(-12)}`;
	const { child, send } = startPi(["--session-id", id, "--msgq-policy", "honor"], { MSGQ_DB: own });
	await send({ id: "s", type: "get_state" });
	const address = `session/${id}`;
	const replyStream = "cli/e2e@test";

	// A record that does not ask for acknowledgement, and cannot start a turn.
	execFileSync(CLI, ["--db", own, "publish", address, "no ack requested", "--type", "note"]);
	await wait(2000);

	// An acknowledgement sent to the session must not be acknowledged back, nor start a turn.
	const ackLike = JSON.stringify({
		v: 1, type: "event", from: "cli/e2e@test", to: address,
		reply_to: 1, event: { kind: "ack", phase: "completed", outcome: "ok" }, meta: { ack: true },
	});
	execFileSync(CLI, ["--db", own, "publish", address, "--json", ackLike]);
	await wait(2500);

	const replies = execFileSync(CLI, ["--db", own, "read", replyStream, "--from-start", "--json"], { encoding: "utf8" }).trim();
	const { data } = await send({ id: "e", type: "get_entries" });
	const assistants = data.entries.filter((e) => e.type === "message" && e.message?.role === "assistant");
	shutdown(child);

	console.log("reply stream:", JSON.stringify(replies), "assistants:", assistants.length);
	report("ack-quiet", replies === "" && assistants.length === 0, `replies=${JSON.stringify(replies)} assistants=${assistants.length}`);
}


async function scenarioDelegate() {
	// An agent session, not a shell, drives the whole delegation.
	const own = resolve(mkdtempSync(resolve(tmpdir(), "msgq-delegate-")), "msgq.db");
	const companion = resolve(REPO, "companion/companion");
	const env = {
		MSGQ_DB: own,
		MSGQ_BIN: CLI,
		MSGQ_COMPANION: companion,
		MSGQ_SPAWN_EXTRA: `-e ${resolve(REPO, "extension")}`,
	};
	const { child, send, waitForEvent } = startPi(["--name", "planner", "--msgq-policy", "honor"], env, {
		noTools: false,
	});
	await send({ id: "s", type: "get_state" });

	child.stdin.write(
		`${JSON.stringify({
			id: "p",
			type: "prompt",
			message:
				"Use the companions tool to spawn a companion named helper. Then use the peers tool with action 'ask' to ask it: What is 2+2? Reply with just the number. Finally, report what it answered.",
		})}\n`,
	);

	await waitForEvent("agent_settled", 600_000);
	await wait(1500);

	const { data } = await send({ id: "e", type: "get_entries" });
	const asked = data.entries.some(
		(entry) =>
			entry.type === "message" &&
			entry.message?.role === "assistant" &&
			JSON.stringify(entry.message.content).includes('"ask"'),
	);
	const answered = data.entries.some(
		(entry) =>
			entry.type === "message" &&
			entry.message?.role === "toolResult" &&
			entry.message?.toolName === "peers" &&
			JSON.stringify(entry.message.content).includes("4"),
	);
	console.log("asked:", asked, "answered:", answered);

	shutdown(child);
	try {
		execFileSync(companion, ["close", "helper"], { env: { ...process.env, ...env } });
	} catch {
		/* already gone */
	}
	report("delegate", asked && answered, `asked=${asked} answered=${answered}`);
}

async function scenarioCommandSurface() {
	const { child, send } = startPi();
	const { data } = await send({ id: "c", type: "get_commands" });
	// Other extensions come from the user's own config. A directory `-e` resolves through the
	// package manifest, so what this checkout contributes is a path that is the extension
	// directory or lives inside it — the extension itself, and the skill it ships.
	const ours = data.commands.filter((entry) => {
		const path = String(entry.sourceInfo?.path ?? "");
		return path === EXTENSION || path.startsWith(`${EXTENSION}/`);
	});
	const commands = ours
		.filter((entry) => entry.source === "extension")
		.map((entry) => entry.name)
		.sort();
	const skills = ours
		.filter((entry) => entry.source === "skill")
		.map((entry) => entry.name)
		.sort();
	const described = ours.every((entry) => String(entry.description ?? "").length > 20);
	shutdown(child);

	const ok =
		JSON.stringify(commands) === JSON.stringify(["companions", "peer", "peers", "reply"]) &&
		JSON.stringify(skills) === JSON.stringify(["skill:msgq-delegate", "skill:msgq-notify"]) &&
		described;
	report(
		"command-surface",
		ok,
		`commands=${JSON.stringify(commands)} skills=${JSON.stringify(skills)} described=${described}`,
	);
}

async function scenarioCommandNoPeers() {
	// A dedicated bus: the only participant is the session itself.
	const own = resolve(mkdtempSync(resolve(tmpdir(), "msgq-nopeers-")), "msgq.db");
	const session = startPi([], { MSGQ_DB: own });
	await session.send({ id: "s", type: "get_state" });

	const before = await session.send({ id: "s", type: "get_state" });
	const disposition = await runCommand(session, "/peers");
	const said = notices(session);
	shutdown(session.child);

	const ok = disposition.disposition === "handled" && said.some((m) => m.includes("no other participant"));
	report("command-no-peers", ok, `disposition=${disposition.disposition} said=${JSON.stringify(said)}`);
}

async function scenarioCommandPickAndSend() {
	const own = resolve(mkdtempSync(resolve(tmpdir(), "msgq-pick-")), "msgq.db");
	if (!announcePeer(own, "reviewer", "peer-one")) report("command-pick-and-send", false, "could not make a peer");

	const session = startPi([], { MSGQ_DB: own });
	await session.send({ id: "s", type: "get_state" });

	// A command that opens a dialog only answers its prompt once the dialog is
	// closed, so the dialogs are answered while the prompt is still in flight.
	const pending = session.send({ id: "p1", type: "prompt", message: "/peers" });
	const choice = await answerDialog(session, "select", (request) => ({ value: request.options[0] }));
	const selected = choice?.options?.[0];
	const composed = await answerDialog(session, "editor", { value: "hello from the picker" });
	const disposition = (await pending).data ?? {};
	await wait(1500);

	const records = readStream("session/reviewer", own);

	// Abandoning the composition sends nothing either.
	const abandoning = session.send({ id: "p2", type: "prompt", message: "/peers" });
	await answerDialog(session, "select", (request) => ({ value: request.options[0] }));
	const abandoned = await answerDialog(session, "editor", { cancelled: true });
	await abandoning;
	await wait(1200);
	const afterAbandon = readStream("session/reviewer", own);
	const said = notices(session);
	shutdown(session.child);

	const ok =
		disposition.disposition === "handled" &&
		String(selected).startsWith("session/reviewer") &&
		Boolean(composed) &&
		Boolean(abandoned) &&
		records.length === 1 &&
		records[0].message === "hello from the picker" &&
		records[0].type === "prompt" &&
		records[0].from.startsWith("session/") &&
		afterAbandon.length === 1 &&
		said.some((m) => m.includes("sent to session/reviewer"));
	report(
		"command-pick-and-send",
		ok,
		`selected=${JSON.stringify(selected)} composed=${Boolean(composed)} abandoned=${Boolean(abandoned)} records=${records.length} afterAbandon=${afterAbandon.length} said=${JSON.stringify(said)}`,
	);
}

async function scenarioCommandPeerDirect() {
	// Two sessions on one bus: one sends with a command, the other acts on it.
	const own = resolve(mkdtempSync(resolve(tmpdir(), "msgq-cmdpeer-")), "msgq.db");
	const target = startPi(["--msgq-policy", "honor"], { MSGQ_DB: own });
	const targetState = await target.send({ id: "s", type: "get_state" });
	const targetAddress = `session/${targetState.data.sessionId}`;

	const sender = startPi([], { MSGQ_DB: own });
	const senderState = await sender.send({ id: "s", type: "get_state" });
	const senderAddress = `session/${senderState.data.sessionId}`;
	console.log("sender:", senderAddress, "target:", targetAddress);

	const disposition = await runCommand(sender, `/peer ${targetAddress} "Reply with exactly: pong"`, 500);
	await target.waitForEvent("agent_settled", 90_000);
	await wait(1500);

	const { data } = await target.send({ id: "e", type: "get_entries" });
	const { custom, assistants } = split(data.entries);
	const delivered = custom.find((entry) => String(entry.content).includes("Reply with exactly: pong"));

	shutdown(sender.child);
	shutdown(target.child);

	const ok =
		disposition.disposition === "handled" &&
		Boolean(delivered) &&
		String(delivered?.content).includes(senderAddress) &&
		assistants.length >= 1;
	report(
		"command-peer-direct",
		ok,
		`delivered=${Boolean(delivered)} from=${senderAddress} assistants=${assistants.length}`,
	);
}

async function scenarioCommandPeerCompose() {
	const own = resolve(mkdtempSync(resolve(tmpdir(), "msgq-compose-")), "msgq.db");
	const session = startPi([], { MSGQ_DB: own });
	await session.send({ id: "s", type: "get_state" });

	// An address but no message opens the editor.
	const sending = session.send({ id: "p1", type: "prompt", message: "/peer session/target" });
	const composed = await answerDialog(session, "editor", { value: "composed by hand" });
	await sending;
	await wait(1500);
	const afterSend = readStream("session/target", own);

	// Abandoning the composition sends nothing.
	const abandoning = session.send({ id: "p2", type: "prompt", message: "/peer session/target" });
	const cancelled = await answerDialog(session, "editor", { cancelled: true });
	await abandoning;
	await wait(1200);
	const afterCancel = readStream("session/target", own);

	shutdown(session.child);

	const ok =
		Boolean(composed) &&
		Boolean(cancelled) &&
		afterSend.length === 1 &&
		afterSend[0].message === "composed by hand" &&
		afterCancel.length === 1;
	report(
		"command-peer-compose",
		ok,
		`composed=${Boolean(composed)} cancelled=${Boolean(cancelled)} sent=${afterSend.length} afterCancel=${afterCancel.length}`,
	);
}

async function scenarioCommandPeerPick() {
	const own = resolve(mkdtempSync(resolve(tmpdir(), "msgq-peerpick-")), "msgq.db");
	if (!announcePeer(own, "reviewer", "peer-two")) report("command-peer-pick", false, "could not make a peer");

	const session = startPi([], { MSGQ_DB: own });
	await session.send({ id: "s", type: "get_state" });

	// A quoted message means no address, so the peers are offered instead.
	const pending = session.send({ id: "p1", type: "prompt", message: '/peer "picked message"' });
	const choice = await answerDialog(session, "select", (request) => ({ value: request.options[0] }));
	const disposition = (await pending).data ?? {};
	await wait(1500);

	const records = readStream("session/reviewer", own);
	shutdown(session.child);

	const ok =
		disposition.disposition === "handled" &&
		Boolean(choice) &&
		records.length === 1 &&
		records[0].message === "picked message";
	report(
		"command-peer-pick",
		ok,
		`offered=${Boolean(choice)} records=${records.length} message=${JSON.stringify(records[0]?.message)}`,
	);
}

async function scenarioCommandReply() {
	const own = resolve(mkdtempSync(resolve(tmpdir(), "msgq-cmdreply-")), "msgq.db");
	const session = startPi([], { MSGQ_DB: own });
	const state = await session.send({ id: "s", type: "get_state" });
	const address = `session/${state.data.sessionId}`;

	// Nothing has been received yet, so there is nothing to answer.
	const early = await runCommand(session, "/reply too early");
	const earlySaid = notices(session);
	const earlyRecords = readStream("cli/e2e@test", own);

	// Now receive one, and answer it.
	const offset = Number(
		execFileSync(CLI, ["--db", own, "publish", address, "what do you think?", "--from", "cli/e2e@test"], {
			encoding: "utf8",
		}).trim(),
	);
	await wait(2000);
	await runCommand(session, "/reply on it");

	const replies = readStream("cli/e2e@test", own);
	shutdown(session.child);

	const ok =
		early.disposition === "handled" &&
		earlySaid.some((m) => m.includes("nothing to reply to")) &&
		earlyRecords.length === 0 &&
		replies.length === 1 &&
		replies[0].message === "on it" &&
		replies[0].to === "cli/e2e@test" &&
		Number(replies[0].reply_to) === offset &&
		replies[0].from === address;
	report(
		"command-reply",
		ok,
		`early=${JSON.stringify(earlySaid)} replies=${replies.length} reply_to=${replies[0]?.reply_to} offset=${offset}`,
	);
}

async function scenarioCommandAbsentRecipient() {
	const own = resolve(mkdtempSync(resolve(tmpdir(), "msgq-absent-")), "msgq.db");
	const session = startPi([], { MSGQ_DB: own });
	const state = await session.send({ id: "s", type: "get_state" });
	const self = `session/${state.data.sessionId}`;

	const before = session.ui.length;
	const disposition = await runCommand(session, "/peer session/nobody-here ping");
	const said = notices(session, before);
	const records = readStream("session/nobody-here", own);

	const alive = await session.send({ id: "a", type: "get_state" });
	shutdown(session.child);

	const ok =
		disposition.disposition === "handled" &&
		records.length === 1 &&
		records[0].to === "session/nobody-here" &&
		records[0].from === self &&
		said.some((m) => m.includes("nothing is listening at session/nobody-here")) &&
		alive.success === true;
	report(
		"command-absent-recipient",
		ok,
		`records=${records.length} said=${JSON.stringify(said)} alive=${alive.success}`,
	);
}

async function scenarioCommandMe() {
	// Under honor a self-addressed prompt would start a turn, but for the guard.
	const own = resolve(mkdtempSync(resolve(tmpdir(), "msgq-cmdme-")), "msgq.db");
	const session = startPi(["--msgq-policy", "honor"], { MSGQ_DB: own });
	const state = await session.send({ id: "s", type: "get_state" });
	const self = `session/${state.data.sessionId}`;

	const disposition = await runCommand(session, '/peer me "a note to myself"');
	const records = readStream(self, own);
	const { data } = await session.send({ id: "e", type: "get_entries" });
	const { custom, assistants } = split(data.entries);
	shutdown(session.child);

	const delivered = custom.some((entry) => String(entry.content).includes("a note to myself"));
	const ok =
		disposition.disposition === "handled" &&
		records.length === 1 &&
		records[0].to === self &&
		delivered &&
		assistants.length === 0;
	report(
		"command-me",
		ok,
		`to=${records[0]?.to} delivered=${delivered} assistants=${assistants.length}`,
	);
}

async function scenarioCommandIdentity() {
	// The bus cannot be opened, so this is exactly when a human asks who they are.
	const dir = mkdtempSync(resolve(tmpdir(), "msgq-identity-"));
	const blocker = resolve(dir, "not-a-dir");
	writeFileSync(blocker, "x");
	const badPath = resolve(blocker, "msgq.db");

	const session = startPi(["--msgq-policy", "honor", "--msgq-trust", "session/parent"], {
		MSGQ_DB: badPath,
	});
	const state = await session.send({ id: "s", type: "get_state" });
	const self = `session/${state.data.sessionId}`;

	const before = session.ui.length;
	const disposition = await runCommand(session, "/peers me");
	const said = notices(session, before).join("\n");
	shutdown(session.child);

	const ok =
		disposition.disposition === "handled" &&
		said.includes(self) &&
		said.includes("policy") &&
		said.includes("honor") &&
		said.includes("session/parent");
	report("command-identity", ok, `disposition=${disposition.disposition} said=${JSON.stringify(said)}`);
}

async function scenarioCommandNoDialog() {
	// A peer exists, so the pick would be needed, and there is no session to show one.
	const own = resolve(mkdtempSync(resolve(tmpdir(), "msgq-nodialog-")), "msgq.db");
	if (!announcePeer(own, "reviewer", "peer-three")) report("command-no-dialog", false, "could not make a peer");

	const listing = oneShot("/peers", {}, own);
	const pick = oneShot("/peer", {}, own);
	const reply = oneShot("/reply hi", {}, own);

	const published = readStream("session/reviewer", own);
	const ok =
		listing.code === 0 &&
		listing.err.includes("session/reviewer") &&
		listing.err.includes("no dialog can be shown") &&
		pick.code === 0 &&
		pick.err.includes("no dialog can be shown") &&
		reply.code === 0 &&
		reply.err.includes("nothing to reply to") &&
		published.length === 0;
	report(
		"command-no-dialog",
		ok,
		`listing=${JSON.stringify(listing.err.trim())} published=${published.length}`,
	);
}

/**
 * A fake herdr, so a test never depends on a live window manager. It answers
 * `agent list` with $FAKE_HERDR_AGENTS and nothing else.
 */
function fakeHerdr() {
	const dir = mkdtempSync(resolve(tmpdir(), "msgq-fakeherdr-"));
	const bin = resolve(dir, "herdr");
	writeFileSync(
		bin,
		'#!/usr/bin/env bash\n[ "$1 $2" = "agent list" ] && cat "$FAKE_HERDR_AGENTS" && exit 0\nexit 1\n',
	);
	chmodSync(bin, 0o755);
	return { dir, bin };
}

/** Two named agents, one of them on the bus and one not, plus an unnamed one. */
function fakeAgents() {
	const file = resolve(mkdtempSync(resolve(tmpdir(), "msgq-agents-")), "agents.json");
	writeFileSync(
		file,
		JSON.stringify({
			result: {
				agents: [
					{ name: "reviewer", agent_status: "working", cwd: "/src/api", tab_id: "w1:t1", pane_id: "w1:p1" },
					{ name: "lonely", agent_status: "idle", cwd: "/src/web", tab_id: "w1:t2", pane_id: "w1:p2" },
					{ agent_status: "idle", cwd: "/src/noname", tab_id: "w1:t3", pane_id: "w1:p3" },
				],
			},
		}),
	);
	return file;
}

async function scenarioCompanionList() {
	const own = resolve(mkdtempSync(resolve(tmpdir(), "msgq-complist-")), "msgq.db");
	const herdr = fakeHerdr();
	const agents = fakeAgents();
	// Only `reviewer` is on the bus; `lonely` is a tab that is not listening, and the
	// third has no name at all.
	const leased = announcePeer(own, "reviewer", "peer-list", 120);

	const env = {
		...process.env,
		MSGQ_DB: own,
		MSGQ_BIN: CLI,
		HERDR_ENV: "1",
		FAKE_HERDR_AGENTS: agents,
		PATH: `${herdr.dir}:${process.env.PATH}`,
	};
	const script = resolve(REPO, "companion/companion");
	const asJson = spawnSync(script, ["list", "--json"], { env, encoding: "utf8" });
	const asText = spawnSync(script, ["list"], { env, encoding: "utf8" });

	// Outside herdr, and with no herdr on PATH, it says so instead of dying obscurely.
	const outside = spawnSync(script, ["list"], { env: { ...env, HERDR_ENV: "" }, encoding: "utf8" });
	const missing = spawnSync(script, ["list"], {
		env: { ...env, PATH: "/usr/bin:/bin" },
		encoding: "utf8",
	});

	let parsed = {};
	try {
		parsed = JSON.parse(asJson.stdout.trim());
	} catch {}
	const entries = parsed.companions ?? [];

	const ok =
		leased &&
		asJson.status === 0 &&
		entries.length === 1 &&
		entries[0].name === "reviewer" &&
		entries[0].address === "session/reviewer" &&
		entries[0].status === "working" &&
		entries[0].cwd === "/src/api" &&
		asText.stdout.includes("reviewer") &&
		!asText.stdout.includes("lonely") &&
		outside.status !== 0 &&
		outside.stderr.includes("herdr") &&
		missing.status !== 0 &&
		missing.stderr.includes("herdr") &&
		missing.stderr.includes("PATH");
	report(
		"companion-list",
		ok,
		`entries=${JSON.stringify(entries)} outside=${JSON.stringify(outside.stderr.trim())} missing=${JSON.stringify(missing.stderr.trim())}`,
	);
}

async function scenarioCompanionModel() {
	// Nothing is spawned: `--dry-run` prints the command a companion would be started with, which is
	// where a chosen model and provider have to appear. Credentials are not this command's business.
	const own = resolve(mkdtempSync(resolve(tmpdir(), "msgq-model-")), "msgq.db");
	const script = resolve(REPO, "companion/companion");
	const env = { ...process.env, MSGQ_BIN: CLI, MSGQ_DB: own, HERDR_ENV: "" };

	const forwarded = spawnSync(
		script,
		[
			"spawn",
			"reviewer",
			"--parent",
			"session/planner",
			"--model",
			"lab/qwen3-30b",
			"--provider",
			"lab",
			"--thinking",
			"xhigh",
			"--dry-run",
		],
		{ env, encoding: "utf8" },
	);
	const started = forwarded.stdout.split("\n").find((line) => line.includes("agent start")) ?? "";
	const carriesModel = started.includes("--model lab/qwen3-30b");
	const carriesProvider = started.includes("--provider lab");
	const carriesThinking = started.includes("--thinking xhigh");

	// A provider only narrows model lookup, so it cannot stand alone: pi says the same thing, and
	// failing here names the fix instead of leaving a tab that never becomes reachable.
	const refused = spawnSync(
		script,
		["spawn", "reviewer", "--parent", "session/planner", "--provider", "lab", "--dry-run"],
		{ env, encoding: "utf8" },
	);
	// A level pi does not know is refused before a tab exists, naming the choices.
	const badLevel = spawnSync(
		script,
		["spawn", "reviewer", "--parent", "session/planner", "--thinking", "turbo", "--dry-run"],
		{ env, encoding: "utf8" },
	);

	report(
		"companion-model",
		forwarded.status === 0 &&
			carriesModel &&
			carriesProvider &&
			carriesThinking &&
			refused.status !== 0 &&
			refused.stderr.includes("needs --model") &&
			badLevel.status !== 0 &&
			badLevel.stderr.includes("off, minimal, low, medium, high, xhigh, max"),
		`model=${carriesModel} provider=${carriesProvider} thinking=${carriesThinking} refusedProvider=${refused.status !== 0} refusedLevel=${badLevel.status !== 0}`,
	);
}

async function scenarioCommandCompanions() {
	const own = resolve(mkdtempSync(resolve(tmpdir(), "msgq-cmdcomp-")), "msgq.db");
	const herdr = fakeHerdr();
	const agents = fakeAgents();
	announcePeer(own, "reviewer", "peer-comp", 120);

	const session = startPi([], {
		MSGQ_DB: own,
		MSGQ_COMPANION: resolve(REPO, "companion/companion"),
		HERDR_ENV: "1",
		FAKE_HERDR_AGENTS: agents,
		PATH: `${herdr.dir}:${process.env.PATH}`,
	});
	await session.send({ id: "s", type: "get_state" });

	const before = session.ui.length;
	const disposition = await runCommand(session, "/companions");
	const said = notices(session, before).join("\n");
	shutdown(session.child);

	const ok =
		disposition.disposition === "handled" &&
		said.includes("reviewer") &&
		said.includes("working") &&
		said.includes("/src/api") &&
		!said.includes("lonely");
	report("command-companions", ok, `said=${JSON.stringify(said)}`);
}

async function scenarioCommandWithoutHerdr() {
	// herdr missing: only /companions notices. The bus commands are unaffected.
	const own = resolve(mkdtempSync(resolve(tmpdir(), "msgq-noherdr-")), "msgq.db");
	const session = startPi([], {
		MSGQ_DB: own,
		MSGQ_COMPANION: resolve(REPO, "companion/companion"),
		HERDR_ENV: "",
	});
	await session.send({ id: "s", type: "get_state" });

	const before = session.ui.length;
	await runCommand(session, "/companions");
	const complained = notices(session, before).join("\n");

	const beforePeer = session.ui.length;
	await runCommand(session, "/peer session/someone hello");
	const saidPeer = notices(session, beforePeer).join("\n");
	const records = readStream("session/someone", own);

	const alive = await session.send({ id: "a", type: "get_state" });
	shutdown(session.child);

	const ok =
		complained.includes("herdr") &&
		saidPeer.includes("sent to session/someone") &&
		records.length === 1 &&
		records[0].message === "hello" &&
		alive.success === true;
	report(
		"command-without-herdr",
		ok,
		`complained=${JSON.stringify(complained)} peer=${JSON.stringify(saidPeer)} records=${records.length}`,
	);
}

async function scenarioReferenceQuoted() {
	const own = resolve(mkdtempSync(resolve(tmpdir(), "msgq-ref-")), "msgq.db");
	const session = startPi([], { MSGQ_DB: own });
	const state = await session.send({ id: "s", type: "get_state" });
	const address = `session/${state.data.sessionId}`;

	// The referenced record, in the stream named by the sender that will refer to it.
	const id = execFileSync(
		CLI,
		["--db", own, "publish", "cli/asker", "review this diff", "--from", "cli/asker", "--print-id"],
		{ encoding: "utf8" },
	).trim();

	// The referencing record, sent from the same address, so the reference is exact.
	// A note, so the default display policy shows it without starting a turn.
	execFileSync(CLI, [
		"--db", own, "publish", address, "looks fine to me",
		"--from", "cli/asker", "--type", "note", "--reply-to", id,
	]);
	await wait(2500);

	const { data } = await session.send({ id: "e", type: "get_entries" });
	const { custom } = split(data.entries);
	const content = String(custom[0]?.content ?? "");
	shutdown(session.child);

	const expected = `[about ${id} from cli/asker: "review this diff"]`;
	const ok = custom.length === 1 && content.includes(expected);
	report("reference-quoted", ok, `id=${id} content=${JSON.stringify(content)}`);
}

async function scenarioReferenceAck() {
	const own = resolve(mkdtempSync(resolve(tmpdir(), "msgq-refack-")), "msgq.db");
	const session = startPi([], { MSGQ_DB: own });
	const state = await session.send({ id: "s", type: "get_state" });
	const address = `session/${state.data.sessionId}`;

	// The record being acknowledged, in the stream named by the address that acknowledges it.
	const id = execFileSync(
		CLI,
		["--db", own, "publish", "cli/asker", "run the suite", "--from", "cli/asker", "--print-id"],
		{ encoding: "utf8" },
	).trim();
	const offset = Number(id.split("#")[1]);

	const ack = JSON.stringify({
		v: 1,
		type: "event",
		from: "cli/asker",
		to: address,
		reply_to: offset,
		event: { kind: "ack", phase: "completed", outcome: "ok" },
	});
	execFileSync(CLI, ["--db", own, "publish", address, "--json", ack]);
	await wait(2500);

	const { data } = await session.send({ id: "e", type: "get_entries" });
	const { custom } = split(data.entries);
	const content = String(custom[0]?.content ?? "");
	shutdown(session.child);

	// An acknowledgement references the same way a reply does, so progress is read the
	// same way an answer is.
	const expected = `[about ${id} from cli/asker: "run the suite"]`;
	const ok = content.includes(expected);
	report("reference-ack", ok, `id=${id} content=${JSON.stringify(content)}`);
}

async function scenarioReferenceDangling() {
	const own = resolve(mkdtempSync(resolve(tmpdir(), "msgq-refgone-")), "msgq.db");
	const session = startPi([], { MSGQ_DB: own });
	const state = await session.send({ id: "s", type: "get_state" });
	const address = `session/${state.data.sessionId}`;

	// Nothing is at that offset in the sender's stream.
	execFileSync(CLI, [
		"--db", own, "publish", address, "who is this?",
		"--from", "cli/asker", "--type", "note", "--reply-to", "cli/asker#99",
	]);
	await wait(2500);

	const { data } = await session.send({ id: "e", type: "get_entries" });
	const { custom } = split(data.entries);
	const content = String(custom[0]?.content ?? "");
	const alive = await session.send({ id: "a", type: "get_state" });
	shutdown(session.child);

	const ok =
		custom.length === 1 &&
		content.includes("[about cli/asker#99]") &&
		!content.includes('from cli/asker:') &&
		alive.success === true;
	report("reference-dangling", ok, `content=${JSON.stringify(content)} alive=${alive.success}`);
}

async function scenarioNotifyWakes() {
	// The skill's pattern, walked without an agent in the loop: a process that is not the
	// session publishes to it, from its own address, and the session is woken.
	const own = resolve(mkdtempSync(resolve(tmpdir(), "msgq-notify-")), "msgq.db");
	const session = startPi(["--msgq-policy", "honor"], { MSGQ_DB: own });
	const state = await session.send({ id: "s", type: "get_state" });
	const address = `session/${state.data.sessionId}`;
	console.log("session:", address, "policy: honor (a job wakes it)");

	execFileSync(CLI, [
		"--db", own, "publish", address, "the background job finished",
		"--from", "cli/bg@test", "--type", "steer",
	]);
	await session.waitForEvent("agent_settled", 90_000);
	await wait(1500);

	const { data } = await session.send({ id: "e", type: "get_entries" });
	const { custom, assistants } = split(data.entries);
	const woken = custom.some((entry) => String(entry.content).includes("the background job finished"));
	shutdown(session.child);

	report(
		"notify-wakes",
		woken && assistants.length >= 1,
		`woken=${woken} assistants=${assistants.length}`,
	);
}

/** The text the peers tool returned, from the session's own transcript. */
async function peersToolText(session) {
	const { data } = await session.send({ id: "e", type: "get_entries" });
	return data.entries
		.filter(
			(entry) =>
				entry.type === "message" &&
				entry.message?.role === "toolResult" &&
				entry.message?.toolName === "peers",
		)
		.flatMap((entry) => entry.message.content ?? [])
		.filter((block) => block.type === "text")
		.map((block) => block.text)
		.join("\n");
}

async function scenarioDelegateIdentity() {
	// The tool path needs a model: nothing in RPC calls a tool directly.
	const asked =
		'Call the peers tool with action "list". Then reply with the first line of what it returned, verbatim.';

	const own = resolve(mkdtempSync(resolve(tmpdir(), "msgq-ident-")), "msgq.db");
	const named = startPi(["--name", "planner"], { MSGQ_DB: own }, { noTools: false });
	named.child.stdin.write(`${JSON.stringify({ id: "p", type: "prompt", message: asked })}\n`);
	await named.waitForEvent("agent_settled", 180_000);
	await wait(1500);
	const namedText = await peersToolText(named);
	shutdown(named.child);

	// The same tool where the bus cannot be read at all: the session still knows who it is.
	const dir = mkdtempSync(resolve(tmpdir(), "msgq-identbad-"));
	const blocker = resolve(dir, "not-a-dir");
	writeFileSync(blocker, "x");
	const lonely = startPi(["--name", "lonely"], { MSGQ_DB: resolve(blocker, "msgq.db") }, {
		noTools: false,
	});
	lonely.child.stdin.write(`${JSON.stringify({ id: "p", type: "prompt", message: asked })}\n`);
	await lonely.waitForEvent("agent_settled", 180_000);
	await wait(1500);
	const lonelyText = await peersToolText(lonely);
	shutdown(lonely.child);

	const ok =
		namedText.includes("session/planner") &&
		namedText.includes("  bus ") &&
		namedText.includes("  depth ") &&
		// The roster is the tab-separated `msgq sessions` output, and it comes after.
		namedText.includes("\t") &&
		namedText.indexOf("  bus ") < namedText.indexOf("\t") &&
		lonelyText.includes("session/lonely") &&
		lonelyText.includes("  depth ");
	report(
		"delegate-identity",
		ok,
		`named=${JSON.stringify(namedText.slice(0, 180))} lonely=${JSON.stringify(lonelyText.slice(0, 140))}`,
	);
}

async function scenarioDelegateDepth() {
	// A live spawn, and the delegate asked what it is allowed to do. It is the one thing
	// the dry run cannot show: whether the cap actually reaches the child.
	const own = resolve(mkdtempSync(resolve(tmpdir(), "msgq-depth-")), "msgq.db");
	const companion = resolve(REPO, "companion/companion");
	const env = {
		...process.env,
		MSGQ_DB: own,
		MSGQ_BIN: CLI,
		MSGQ_COMPANION: companion,
		MSGQ_SPAWN_EXTRA: `-e ${resolve(REPO, "extension")}`,
	};
	execFileSync(companion, ["spawn", "helper", "--parent", "session/probe", "--json"], {
		env,
		encoding: "utf8",
	});

	let answer = "";
	try {
		answer = execFileSync(
			CLI,
			[
				"--db", own, "ask", "session/helper",
				"Run the peers tool with action 'list', then answer with just the depth line from its output.",
				"--from", "session/probe", "--timeout", "240",
			],
			{ encoding: "utf8" },
		);
	} catch (error) {
		answer = String(error.stdout ?? "");
	}
	try {
		execFileSync(companion, ["close", "helper"], { env, encoding: "utf8" });
	} catch {
		/* already gone */
	}

	// Depth 1, and the cap it was told about, rather than discovering it by being refused.
	const ok = /1 of 1/.test(answer);
	report("delegate-depth", ok, `answer=${JSON.stringify(answer.trim().slice(0, 200))}`);
}

async function scenarioDelegateTrust() {
	// A spawned companion's trusted set, read from its own acknowledgements rather than by
	// waiting on a model: `delivery=turn` means it acted, `delivery=display` means it did not.
	const own = resolve(mkdtempSync(resolve(tmpdir(), "msgq-dtrust-")), "msgq.db");
	const companion = resolve(REPO, "companion/companion");
	const env = {
		...process.env,
		MSGQ_DB: own,
		MSGQ_BIN: CLI,
		MSGQ_COMPANION: companion,
		MSGQ_SPAWN_EXTRA: `-e ${resolve(REPO, "extension")}`,
	};
	// Trusting its parent and one other sender; following a topic it is not told to trust.
	execFileSync(
		companion,
		[
			"spawn", "helper", "--parent", "session/probe",
			"--accept", "cli/second@test", "--topics", "group/watch", "--json",
		],
		{ env, encoding: "utf8" },
	);

	const delivery = (address, from, message) => {
		// `received` is terminal for a display delivery and not for a turn, so `completed` is
		// asked for too: the trusted case then waits for the turn instead of for a timeout.
		const record = JSON.stringify({
			v: 1, type: "prompt", from, to: address, message, meta: { ack: "received,completed" },
		});
		try {
			return execFileSync(
				CLI,
				["--db", own, "publish", address, "--json", record, "--await", "--timeout", "120"],
				{ encoding: "utf8" },
			);
		} catch (error) {
			return String(error.stdout ?? "");
		}
	};

	// A bounded instruction: the trusted case really does start a turn in the companion, and a
	// vague body would leave a real agent free to improvise in this working tree.
	const trusted = delivery("session/helper", "cli/second@test", "Reply with exactly: ok. Nothing else.");
	const untrusted = delivery("session/helper", "cli/third@test", "from a sender it does not");
	// The topic it follows but was never told to trust.
	const watched = delivery("group/watch", "cli/third@test", "watched, not obeyed");

	try {
		execFileSync(companion, ["close", "helper"], { env, encoding: "utf8" });
	} catch {
		/* already gone */
	}

	const ok =
		trusted.includes("delivery=turn") &&
		untrusted.includes("delivery=display") &&
		watched.includes("delivery=display");
	report(
		"delegate-trust",
		ok,
		`trusted=${JSON.stringify(trusted.trim())} untrusted=${JSON.stringify(untrusted.trim())} watched=${JSON.stringify(watched.trim())}`,
	);
}

async function scenarioCompletionOwed() {
	// A peer with no tools cannot answer, so the question it was asked is guaranteed to survive the
	// run: the case the nudge exists for, made deterministic. What is asserted is the mechanism —
	// one entry, once — not the model's compliance.
	const own = resolve(mkdtempSync(resolve(tmpdir(), "msgq-owed-")), "msgq.db");
	const session = startPi(["--msgq-policy", "honor"], { MSGQ_DB: own });
	const state = await session.send({ id: "s", type: "get_state" });
	const address = `session/${state.data.sessionId}`;

	// A question, in the shape `ask` publishes one.
	const record = JSON.stringify({
		v: 1, type: "prompt", from: "cli/asker@test", to: address,
		message: "What is the capital of France?", meta: { reply: true },
	});
	execFileSync(CLI, ["--db", own, "publish", address, "--json", record]);
	await session.waitForEvent("agent_settled", 120_000);

	// The nudge should follow, and a second run with it.
	const entries = async () => (await session.send({ id: `e${Date.now()}`, type: "get_entries" })).data.entries;
	let nodges = [];
	for (let i = 0; i < 60; i++) {
		await wait(1000);
		nodges = (await entries()).filter(
			(entry) => entry.type === "custom_message" && String(entry.content).includes("asked you a question"),
		);
		if (nodges.length > 0) break;
	}
	// Give the run the nudge caused time to finish, so a second one could have appeared.
	await wait(15_000);

	const all = await entries();
	shutdown(session.child);
	const finalNudges = all.filter(
		(entry) => entry.type === "custom_message" && String(entry.content).includes("asked you a question"),
	);
	const assistants = all.filter(
		(entry) => entry.type === "message" && entry.message?.role === "assistant",
	);

	const ok = finalNudges.length === 1 && assistants.length >= 2;
	// It also says so, so how often the convention needs rescuing is knowable from use.
	const said = session.ui.some(
		(record) => record.method === "notify" && String(record.message).includes("asked for the answer owed"),
	);
	report(
		"completion-owed",
		ok && said,
		`nudges=${finalNudges.length} assistants=${assistants.length} said=${said}`,
	);
}

async function scenarioCompletionReported() {
	// Exactly the record the peers tool now sends: a message that asks when the peer's run completes.
	// The peer cannot reply (no tools), so what arrives is the completion and nothing else.
	const own = resolve(mkdtempSync(resolve(tmpdir(), "msgq-done-")), "msgq.db");
	const session = startPi(["--msgq-policy", "honor"], { MSGQ_DB: own });
	const state = await session.send({ id: "s", type: "get_state" });
	const address = `session/${state.data.sessionId}`;

	const record = JSON.stringify({
		v: 1, type: "prompt", from: "cli/asker@test", to: address,
		message: "Say ok, and do not reply.", meta: { ack: "completed" },
	});
	let out = "";
	try {
		out = execFileSync(
			CLI,
			["--db", own, "publish", address, "--json", record, "--await", "--timeout", "120"],
			{ encoding: "utf8" },
		);
	} catch (error) {
		out = String(error.stdout ?? "");
	}
	shutdown(session.child);

	// The report is a notification: the sender learns the run completed, and nothing wakes it.
	const ok = out.includes("completed") && out.includes("outcome=ok");
	report("completion-reported", ok, `out=${JSON.stringify(out.trim())}`);
}

async function scenarioDispatchCommand() {
	// A command is dispatched, not described. `/peers me` is an extension command, so it runs
	// immediately and starts no turn: the proof that this was input rather than content.
	const own = resolve(mkdtempSync(resolve(tmpdir(), "msgq-dispatch-")), "msgq.db");
	const session = startPi(["--msgq-policy", "honor", "--msgq-trust", "session/parent"], {
		MSGQ_DB: own,
	});
	const state = await session.send({ id: "s", type: "get_state" });
	const address = `session/${state.data.sessionId}`;

	const command = (from, body, ack) => {
		const record = { v: 1, type: "command", from, to: address, message: body };
		if (ack) record.meta = { ack };
		const args = ["--db", own, "publish", address, "--json", JSON.stringify(record)];
		// Only waiting when something was asked for: a completion is what --await waits on.
		if (ack) args.push("--await", "--timeout", "60");
		try {
			return execFileSync(CLI, args, { encoding: "utf8" });
		} catch (error) {
			return String(error.stdout ?? "");
		}
	};

	// An extension command runs at once, and starts no turn of its own.
	command("session/parent", "/peers me");
	await wait(2500);
	const ran = session.ui.some(
		(record) => record.method === "notify" && String(record.message).includes("  bus "),
	);

	// A skill command expands into the prompt the session then runs. A second command, published
	// while that run is in flight, must be queued and still run — not gathered into a batch, where
	// it would stop being a command.
	command("session/parent", "/skill:msgq-delegate");
	command("session/parent", "/peers me");
	await session.waitForEvent("agent_settled", 180_000);
	await wait(3000);

	const { data } = await session.send({ id: "e", type: "get_entries" });
	const entries = data.entries;
	const ranReports = session.ui.filter(
		(record) => record.method === "notify" && String(record.message).includes("  bus "),
	).length;
	const noteAt = entries.findIndex(
		(entry) =>
			entry.type === "custom_message" &&
			String(entry.content).includes("the command that follows comes from them"),
	);
	const skillAt = entries.findIndex(
		(entry) =>
			entry.type === "message" &&
			entry.message?.role === "user" &&
			JSON.stringify(entry.message.content).includes("msgq-delegate"),
	);

	// An untrusted sender's command is shown, and nothing runs.
	const refused = command("cli/stranger@test", "/peers me", "received");
	shutdown(session.child);

	// The note is delivered before the command, since a command cannot carry its sender. And the
	// second command ran, which a buffered one could not have.
	const ok =
		ran && ranReports >= 2 && noteAt >= 0 && skillAt > noteAt && refused.includes("delivery=display");
	report(
		"dispatch-command",
		ok,
		`ran=${ran} ranReports=${ranReports} noteAt=${noteAt} skillAt=${skillAt} refused=${JSON.stringify(refused.trim())}`,
	);
}

const scenario = process.argv[2];
if (scenario === "default-note") await scenarioDefaultNote();
else if (scenario === "honor-prompt") await scenarioHonorPrompt();
else if (scenario === "coalesce") await scenarioCoalesce();
else if (scenario === "note-idle") await scenarioNoteIdle();
else if (scenario === "note-busy") await scenarioNoteBusy();
else if (scenario === "self-guard") await scenarioSelfGuard();
else if (scenario === "steer-mid-run") await scenarioSteerMidRun();
else if (scenario === "extra-topic") await scenarioExtraTopic();
else if (scenario === "presence") await scenarioPresence();
else if (scenario === "delegate") await scenarioDelegate();
else if (scenario === "ack-display") await scenarioAckDisplay();
else if (scenario === "ack-honor") await scenarioAckHonor();
else if (scenario === "ack-batch") await scenarioAckBatch();
else if (scenario === "ack-quiet") await scenarioAckQuiet();
else if (scenario === "scoped-write") await scenarioScopedWrite();
else if (scenario === "crash-expiry") await scenarioCrashExpiry();
else if (scenario === "rename") await scenarioRename();
else if (scenario === "squat") await scenarioSquat();
else if (scenario === "presence-off") await scenarioPresenceOff();
else if (scenario === "blocked") await scenarioBlocked();
else if (scenario === "bad-bus") await scenarioBadBus();
else if (scenario === "skip-version") await scenarioSkipVersion();
else if (scenario === "command-surface") await scenarioCommandSurface();
else if (scenario === "command-no-peers") await scenarioCommandNoPeers();
else if (scenario === "command-pick-and-send") await scenarioCommandPickAndSend();
else if (scenario === "command-peer-direct") await scenarioCommandPeerDirect();
else if (scenario === "command-peer-compose") await scenarioCommandPeerCompose();
else if (scenario === "command-peer-pick") await scenarioCommandPeerPick();
else if (scenario === "command-reply") await scenarioCommandReply();
else if (scenario === "command-absent-recipient") await scenarioCommandAbsentRecipient();
else if (scenario === "command-me") await scenarioCommandMe();
else if (scenario === "command-identity") await scenarioCommandIdentity();
else if (scenario === "command-no-dialog") await scenarioCommandNoDialog();
else if (scenario === "companion-list") await scenarioCompanionList();
else if (scenario === "companion-model") await scenarioCompanionModel();
else if (scenario === "command-companions") await scenarioCommandCompanions();
else if (scenario === "command-without-herdr") await scenarioCommandWithoutHerdr();
else if (scenario === "reference-quoted") await scenarioReferenceQuoted();
else if (scenario === "reference-ack") await scenarioReferenceAck();
else if (scenario === "reference-dangling") await scenarioReferenceDangling();
else if (scenario === "notify-wakes") await scenarioNotifyWakes();
else if (scenario === "delegate-identity") await scenarioDelegateIdentity();
else if (scenario === "delegate-depth") await scenarioDelegateDepth();
else if (scenario === "delegate-trust") await scenarioDelegateTrust();
else if (scenario === "completion-owed") await scenarioCompletionOwed();
else if (scenario === "completion-reported") await scenarioCompletionReported();
else if (scenario === "dispatch-command") await scenarioDispatchCommand();
else {
	console.error(
		"usage: node scripts/e2e.mjs <default-note|honor-prompt|coalesce|note-idle|note-busy|self-guard|steer-mid-run|extra-topic|presence|delegate|ack-display|ack-honor|ack-batch|ack-quiet|scoped-write|crash-expiry|rename|squat|presence-off|blocked|bad-bus|skip-version|command-surface|command-no-peers|command-pick-and-send|command-peer-direct|command-peer-compose|command-peer-pick|command-reply|command-absent-recipient|command-me|command-identity|command-no-dialog|companion-list|companion-model|command-companions|command-without-herdr>",
	);
	process.exit(2);
}
