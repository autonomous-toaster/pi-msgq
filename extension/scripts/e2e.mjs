// End-to-end driver for the msgq extension.
//
// Runs pi in RPC mode with `-e <this extension>`, publishes with the msgq CLI,
// and inspects what the extension delivered into the session.
//
//   node scripts/e2e.mjs <default-note|honor-prompt|coalesce|note-busy|bad-bus|skip-version>
//
// Environment overrides: MSGQ_BIN (path to the msgq binary), PI_BIN (pi command),
// MSGQ_DB (bus location).
import { execFileSync, spawn } from "node:child_process";
import { mkdtempSync, writeFileSync } from "node:fs";
import { createRequire } from "node:module";
import { tmpdir } from "node:os";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const require_ = createRequire(import.meta.url);
const honker = require_("@russellthehippo/honker-node");

const HERE = dirname(fileURLToPath(import.meta.url));
const EXTENSION = process.env.MSGQ_EXTENSION ?? resolve(HERE, "..");
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

function startPi(extraArgs = [], envOverrides = {}) {
	const child = spawn(PI, ["--mode", "rpc", "-e", EXTENSION, "-nt", ...extraArgs], {
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
	const { custom, assistants } = split(data.entries);
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
	const streams = Number(native.query("SELECT COUNT(*) AS n FROM _honker_stream")[0].n);
	const leases = Number(
		native.query("SELECT COUNT(*) AS n FROM _honker_locks WHERE name LIKE 'msgq:participant:%'")[0].n,
	);
	native.close();
	shutdown(child);

	const ok = online && streams === 0 && leases === 1;
	report("scoped-write", ok, `online=${online} streams=${streams} participantLeases=${leases}`);
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

	shutdown(child);
	const ok = before?.address === `session/${id}` && after?.alias === "api-worker";
	report("rename", ok, `before=${before?.address} after=${after?.address} alias=${after?.alias}`);
}

async function scenarioSquat() {
	// A dedicated bus and a pinned session id, so we can take the lease first.
	const own = resolve(mkdtempSync(resolve(tmpdir(), "msgq-squat-")), "msgq.db");
	const id = "00000000-0000-7000-8000-000000000002";
	const name = `msgq:participant:session:idle:${id}`;
	const address = `session/${id}`;

	const native = honker.open(own, { watcherPollIntervalMs: 50 });
	const stolen = native.tryLock(name, "other#squat", 60);
	native.close();
	if (!stolen) report("squat", false, "could not pre-take the lease");

	const { child, send } = startPi(["--session-id", id, "--msgq-policy", "honor"], { MSGQ_DB: own });
	await send({ id: "s", type: "get_state" });

	// Consumption must continue even though presence could not register.
	execFileSync(CLI, ["--db", own, "publish", address, "squat delivery check"]);
	await wait(2500);

	const { data } = await send({ id: "e", type: "get_entries" });
	const delivered = data.entries.some(
		(entry) => entry.type === "custom_message" && String(entry.content).includes("squat delivery check"),
	);

	const after = honker.open(own, { watcherPollIntervalMs: 50 });
	const rows = after.query("SELECT owner FROM _honker_locks WHERE name = ?", [name]);
	after.close();
	const owner = rows[0]?.owner;
	shutdown(child);

	const ok = delivered && owner === "other#squat";
	report("squat", ok, `delivered=${delivered} owner=${owner}`);
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
else {
	console.error(
		"usage: node scripts/e2e.mjs <default-note|honor-prompt|coalesce|note-idle|note-busy|self-guard|steer-mid-run|extra-topic|presence|ack-display|ack-honor|ack-batch|ack-quiet|scoped-write|crash-expiry|rename|squat|presence-off|blocked|bad-bus|skip-version>",
	);
	process.exit(2);
}
