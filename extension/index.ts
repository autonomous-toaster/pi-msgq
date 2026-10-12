/**
 * msgq extension: consumes the msgq message bus and delivers records into the
 * session under a subscriber-controlled policy.
 *
 * Writes leases to advertise presence, and nothing else: it never publishes a
 * message. Senders use the `msgq` CLI. Load with:
 *   pi -e ./extension
 */

import { homedir } from "node:os";

import type {
	AgentBeforeSettleEvent,
	AgentBeforeSettleEventResult,
	ExtensionAPI,
	ExtensionContext,
} from "@earendil-works/pi-coding-agent";
import { Type } from "@earendil-works/pi-ai";

import {
	Bus,
	type BusEvent,
	type Subscription,
} from "./src/consumer.ts";
import {
	ConfigError,
	consumerId,
	parsePolicy,
	parsePresence,
	parseOptionalAddress,
	parseTopics,
	parseTrustList,
	parseTtlSeconds,
	type Policy,
	resolveDbPath,
	selfAddress,
	type SessionIdentity,
	subscriptionAddresses,
} from "./src/config.ts";
import {
	type AckDelivery,
	type AckOutcome,
	type AckPhase,
	buildAck,
	isAckRecord,
	parseAckRequest,
	publishSafely,
} from "./src/ack.ts";
import { Coalescer, formatBatch, formatMessage, planDelivery } from "./src/delivery.ts";
import { Presence, PRESENCE_TTL_S } from "./src/presence.ts";
import type { PresenceState } from "./src/presence-text.ts";
import { decodeRecord, type MsgqRecord, recordBody } from "./src/record.ts";
import {
	DEFAULT_ASK_TIMEOUT_S,
	isError,
	planCompanionAction,
	planPeerAction,
} from "./src/tools.ts";
import { renderMsgq } from "./src/render.ts";
import type { MsgqDetails, MsgqItem } from "./src/render-text.ts";

/** Custom-message type used for every delivered msgq message. */
const MESSAGE_TYPE = "msgq";

/** Footer status key. */
const STATUS_KEY = "msgq";

export default function (pi: ExtensionAPI) {
	pi.registerFlag("msgq-topics", {
		description: "Extra msgq addresses to follow, comma-separated",
		type: "string",
	});
	pi.registerFlag("msgq-policy", {
		description: "msgq delivery policy: 'display' (default) or 'honor'",
		type: "string",
	});
	pi.registerFlag("msgq-presence", {
		description: "Register session presence: 'on' (default) or 'off'",
		type: "string",
	});
	pi.registerFlag("msgq-report-to", {
		description: "Default recipient for messages this session sends",
		type: "string",
	});
	pi.registerFlag("msgq-trust", {
		description: "Comma-separated senders allowed to start a turn under 'honor'",
		type: "string",
	});

	pi.registerMessageRenderer(MESSAGE_TYPE, renderMsgq);

	let bus: Bus | null = null;
	let subscriptions: Subscription[] = [];
	let coalescer = new Coalescer();
	let context: ExtensionContext | undefined;
	let presence: Presence | undefined;
	let renewTimer: ReturnType<typeof setInterval> | undefined;
	let identity: SessionIdentity | undefined;
	let self = "";
	let consumer = "";
	let policy: Policy = "display";
	let addresses: string[] = [];
	let skipped = 0;
	/** Senders allowed to start a turn under the honour policy. Empty means no allowlist. */
	let trusted: string[] = [];
	/** Default recipient when a tool call does not name one. */
	let reportTo: string | undefined;
	/** Extra addresses beyond this session's own, kept so a rename can rebuild the set. */
	let extraTopics: string[] = [];
	/** The most recently received message, so a reply can target it. */
	let lastInbound: { from: string; offset: number } | undefined;
	/** How to reach the two binaries the tools delegate to. */
	/** Acknowledged records awaiting a run, so start and completion can be reported. */
	let inFlight: Array<{ offset: number; to: string; phases: AckPhase[] }> = [];
	/** The stop reason of the last assistant message in the current run. */
	let lastStopReason: string | undefined;

	function report(message: string, level: "info" | "warning" | "error" = "info"): void {
		if (context?.hasUI) context.ui.notify(message, level);
	}

	function setStatus(text: string | undefined): void {
		if (context?.hasUI) context.ui.setStatus(STATUS_KEY, text);
	}

	/** Move presence to a new state, reporting a lost lease without failing. */
	function setPresenceState(state: PresenceState): void {
		if (presence && !presence.setState(state)) {
			report(`msgq presence is not registered (${state})`, "warning");
		}
	}

	/**
	 * Publish an acknowledgement. A failure is reported, never fatal: an
	 * acknowledgement must not be able to break consumption.
	 */
	function publishAck(options: {
		to: string;
		replyTo: number;
		phase: AckPhase;
		delivery?: AckDelivery;
		outcome?: AckOutcome;
	}): void {
		const open = bus;
		if (!open) return;
		publishSafely(
			() => open.publish(options.to, buildAck({ from: self, ...options })),
			(error) => report(`msgq could not publish an acknowledgement: ${String(error)}`, "warning"),
		);
	}

	/** Remember that a record now awaits a run, so its later phases can be reported. */
	function trackInFlight(record: MsgqRecord, offset: number, phases: AckPhase[]): void {
		if (!phases.includes("started") && !phases.includes("completed")) return;
		inFlight.push({ offset, to: record.from, phases });
	}

	/** Release every resource. Idempotent across reload and shutdown. */
	function teardown(): void {
		if (renewTimer !== undefined) {
			clearInterval(renewTimer);
			renewTimer = undefined;
		}
		presence?.stop();
		presence = undefined;
		identity = undefined;
		inFlight = [];
		lastStopReason = undefined;
		lastInbound = undefined;
		for (const subscription of subscriptions) subscription.close();
		subscriptions = [];
		coalescer = new Coalescer();
		bus?.close();
		bus = null;
		context = undefined;
		addresses = [];
	}

	/** Disable the extension after a fatal condition, reporting where possible. */
	function disable(reason: string): void {
		report(`msgq disabled: ${reason}`, "error");
		teardown();
		setStatus("msgq: disabled");
	}

	function itemOf(record: MsgqRecord, address: string): MsgqItem {
		return { from: record.from, address, type: record.type, body: recordBody(record) };
	}

	function deliver(
		record: MsgqRecord,
		address: string,
		offset: number,
		subscription: Subscription,
	): void {
		// An acknowledgement is never itself acknowledged.
		const phases = isAckRecord(record) ? undefined : parseAckRequest(record.meta);
		const busy = context ? !context.isIdle() : false;
		const plan = planDelivery(record, policy, self, busy, trusted);

		if (plan === "buffer") {
			// Responsibility is not taken yet: a crash here must redeliver.
			coalescer.add(record, address, offset, () => subscription.save(offset));
			return;
		}

		const turn = plan === "turn";

		// Report and register *before* triggering: a run that starts immediately must
		// already find this record, otherwise its `started` acknowledgement is missed.
		if (phases) {
			publishAck({
				to: record.from,
				replyTo: offset,
				phase: "received",
				delivery: turn ? "turn" : "display",
			});
			if (turn) trackInFlight(record, offset, phases);
		}

		try {
			pi.sendMessage(
				{
					customType: MESSAGE_TYPE,
					content: formatMessage(record, address),
					display: true,
					details: { items: [itemOf(record, address)] } satisfies MsgqDetails,
				},
				{ triggerTurn: turn },
			);
		} catch (error) {
			report(`msgq could not deliver a message: ${String(error)}`, "warning");
		}

		// Responsibility is taken: the record has been acted on, so progress is durable.
		subscription.save(offset);
	}

	/**
	 * (Re)build the subscription set for the current identity. Called on start and on a
	 * rename: the address a session answers to changes, and the subscription has to follow
	 * it or the session stops receiving at the name the roster advertises.
	 */
	function resubscribe(): void {
		const open = bus;
		if (!open || !identity) return;
		for (const subscription of subscriptions) subscription.close();
		const targets = subscriptionAddresses(identity, extraTopics);
		addresses = targets;
		subscriptions = targets.map((address) => {
			const subscription = open.subscribe(
				address,
				consumer,
				(event) => onEvent(address, event, subscription),
				(error) => disable(`bus subscription failed on ${address}: ${String(error)}`),
			);
			return subscription;
		});
	}

	function onEvent(address: string, event: BusEvent, subscription: Subscription): void {
		const decoded = decodeRecord(event.payload);
		if (!decoded.ok) {
			// The envelope cannot be trusted, so it is never acknowledged.
			skipped += 1;
			report(`msgq skipped a record on ${address}: ${decoded.reason}`, "warning");
			return;
		}
		deliver(decoded.record, address, event.offset, subscription);
		// An acknowledgement is not something to reply to, so it does not become the target.
		if (!isAckRecord(decoded.record)) {
			lastInbound = { from: decoded.record.from, offset: event.offset };
		}
	}

	// The agent-facing surface: two tools that delegate, so the wire lives in the CLI and
	// the herdr glue in the companion command, each in one place.
	const msgqBin = process.env.MSGQ_BIN?.trim() || "msgq";
	const companionBin = process.env.MSGQ_COMPANION?.trim() || "companion";

	function toolText(content: string, details?: unknown) {
		return { content: [{ type: "text" as const, text: content }], details };
	}

	/**
	 * Run a planned command and report what it said. Never throws at the model.
	 *
	 * `binEnv` names the variable that points at the binary, because a failure with no
	 * output almost always means it was not found, and that is the caller's fix.
	 */
	async function runPlan(
		plan: { command: string; args: string[] },
		signal: AbortSignal | undefined,
		timeoutMs: number | undefined,
		binEnv: string,
	) {
		try {
			const result = await pi.exec(plan.command, plan.args, { timeout: timeoutMs, signal });
			const out = (result.stdout ?? "").trim();
			const err = (result.stderr ?? "").trim();
			if (result.code !== 0) {
				const detail = err || out;
				if (detail) {
					return toolText(`${plan.command} exited with ${result.code}: ${detail}`);
				}
				return toolText(
					`${plan.command} exited with ${result.code} and said nothing, which usually means ` +
						`it was not found. Check that \"${plan.command}\" is on PATH, or set ${binEnv} to ` +
						"its full path (in a development checkout they are not installed).",
				);
			}
			return toolText(out || "(no output)");
		} catch (error) {
			return toolText(`could not run ${plan.command}: ${String(error)}`);
		}
	}

	const peersTool = {
		name: "peers",
		label: "peers",
		description:
			"Work with other agents on the message bus. 'list' the addresses that are reachable, " +
			"'send' a message to one, 'ask' a question and wait for the answer, or 'reply' to the " +
			"message you most recently received. Use it to delegate to a companion you started, " +
			"or to talk to any other session on the bus.",
		parameters: Type.Object({
			action: Type.Union(
				[Type.Literal("list"), Type.Literal("send"), Type.Literal("ask"), Type.Literal("reply")],
				{ description: "what to do" },
			),
			to: Type.Optional(
				Type.String({ description: "the peer's address, for example session/reviewer" }),
			),
			message: Type.Optional(Type.String({ description: "what to say or ask" })),
			timeoutSeconds: Type.Optional(
				Type.Number({ description: "how long 'ask' waits; 0 waits without a limit" }),
			),
		}),
		async execute(
			_toolCallId: string,
			params: { action: "list" | "send" | "ask" | "reply"; to?: string; message?: string; timeoutSeconds?: number },
			signal: AbortSignal | undefined,
		) {
			const plan = planPeerAction(params, { msgqBin, self, reportTo, lastInbound });
			if (isError(plan)) return toolText(plan.error);
			const seconds = params.action === "ask" ? (params.timeoutSeconds ?? DEFAULT_ASK_TIMEOUT_S) : 0;
			const timeoutMs = seconds > 0 ? seconds * 1000 + 30_000 : undefined;
			return runPlan(plan, signal, timeoutMs, "MSGQ_BIN");
		},
	};
	pi.registerTool(peersTool);

	const companionsTool = {
		name: "companions",
		label: "companions",
		description:
			"Start and stop companion agents. A companion is a real pi process in its own terminal " +
			"tab, reachable at session/<name>. 'spawn' starts one and reports the address to use; " +
			"the task is then simply a message sent with the peers tool. 'close' stops it.",
		parameters: Type.Object({
			action: Type.Union([Type.Literal("spawn"), Type.Literal("close")], {
				description: "what to do",
			}),
			name: Type.Optional(
				Type.String({ description: "a short role name, for example reviewer" }),
			),
			cwd: Type.Optional(
				Type.String({ description: "where the companion runs; defaults to this directory" }),
			),
		}),
		async execute(
			_toolCallId: string,
			params: { action: "spawn" | "close"; name?: string; cwd?: string },
			signal: AbortSignal | undefined,
		) {
			const plan = planCompanionAction(params, { companionBin, self });
			if (isError(plan)) return toolText(plan.error);
			return runPlan(plan, signal, undefined, "MSGQ_COMPANION");
		},
	};
	pi.registerTool(companionsTool);

	pi.on("session_start", async (_event, ctx) => {
		teardown();
		context = ctx;
		skipped = 0;

		// Configuration.
		let extra: string[];
		let presenceOn: boolean;
		let presenceTtl: number;
		try {
			const topicsFlag = pi.getFlag("msgq-topics");
			const policyFlag = pi.getFlag("msgq-policy");
			const presenceFlag = pi.getFlag("msgq-presence");
			const reportFlag = pi.getFlag("msgq-report-to");
			const trustFlag = pi.getFlag("msgq-trust");
			extra = parseTopics(
				(typeof topicsFlag === "string" ? topicsFlag : undefined) ??
					process.env.MSGQ_TOPICS,
			);
			policy = parsePolicy(
				(typeof policyFlag === "string" ? policyFlag : undefined) ??
					process.env.MSGQ_POLICY,
			);
			presenceOn = parsePresence(
				(typeof presenceFlag === "string" ? presenceFlag : undefined) ??
					process.env.MSGQ_PRESENCE,
			);
			presenceTtl = parseTtlSeconds(process.env.MSGQ_PRESENCE_TTL, PRESENCE_TTL_S);
			reportTo = parseOptionalAddress(
				(typeof reportFlag === "string" ? reportFlag : undefined) ??
					process.env.MSGQ_PARENT,
			);
			trusted = parseTrustList(
				(typeof trustFlag === "string" ? trustFlag : undefined) ??
					process.env.MSGQ_TRUST,
			);
		} catch (error) {
			disable(error instanceof ConfigError ? error.message : String(error));
			return;
		}

		// Identity.
		identity = {
			alias: pi.getSessionName() ?? ctx.sessionManager.getSessionName(),
			sessionId: ctx.sessionManager.getSessionId(),
		};
		self = selfAddress(identity);
		consumer = consumerId(identity);
		extraTopics = extra;

		// Bus.
		const dbPath = resolveDbPath(process.env, homedir());
		try {
			bus = new Bus(dbPath);
			resubscribe();
		} catch (error) {
			disable(`cannot open ${dbPath}: ${String(error)}`);
			return;
		}

		// Presence: a scoped write exception. A failure here is never fatal.
		if (presenceOn && bus) {
			presence = new Presence(bus, {
				kind: "session",
				id: identity.sessionId,
				owner: consumer,
				state: ctx.isIdle() ? "idle" : "busy",
				ttlS: presenceTtl,
			});
			if (!presence.start()) {
				report("msgq presence is not registered (lease held elsewhere)", "warning");
			}
			const renewMs = Math.max(1, Math.floor(presenceTtl / 3)) * 1000;
			renewTimer = setInterval(() => {
				if (presence && !presence.renew()) {
					report("msgq presence was lost and could not be re-registered", "warning");
				}
			}, renewMs);
			renewTimer.unref?.();
		}

		setStatus(`msgq: ${addresses.length} (${policy})`);
		report(`msgq: following ${addresses.join(", ")} as ${consumer} (${policy})`);
	});

	pi.on("agent_before_settle", async (_event: AgentBeforeSettleEvent) => {
		if (coalescer.empty) return;
		const batch = coalescer.drain();

		// Responsibility is taken here: the batch is about to be injected, so it can
		// no longer be lost, and its progress is persisted.
		for (const item of batch) {
			item.commit();
			const phases = isAckRecord(item.record) ? undefined : parseAckRequest(item.record.meta);
			if (phases) {
				publishAck({
					to: item.record.from,
					replyTo: item.offset,
					phase: "received",
					delivery: "queued",
				});
				trackInFlight(item.record, item.offset, phases);
			}
		}

		const details: MsgqDetails = {
			items: batch.map((item) => itemOf(item.record, item.address)),
		};
		const result: AgentBeforeSettleEventResult = {
			entries: [
				{
					type: "custom_message",
					customType: MESSAGE_TYPE,
					content: formatBatch(batch),
					display: true,
					details,
				},
			],
			continue: true,
		};
		return result;
	});

	pi.on("agent_start", async () => {
		lastStopReason = undefined;
		setPresenceState("busy");
		for (const entry of inFlight) {
			if (entry.phases.includes("started")) {
				publishAck({ to: entry.to, replyTo: entry.offset, phase: "started" });
			}
		}
	});

	pi.on("agent_settled", async (_event, ctx) => {
		setPresenceState(ctx?.isIdle?.() === false ? "busy" : "idle");
		const outcome: AckOutcome =
			lastStopReason === "error" ? "error" : lastStopReason === "aborted" ? "aborted" : "ok";
		for (const entry of inFlight) {
			if (entry.phases.includes("completed")) {
				publishAck({
					to: entry.to,
					replyTo: entry.offset,
					phase: "completed",
					outcome,
				});
			}
		}
		inFlight = [];
	});

	pi.on("message_end", async (event) => {
		const message = (event as { message?: { role?: string; stopReason?: string } }).message;
		if (message?.role === "assistant" && typeof message.stopReason === "string") {
			lastStopReason = message.stopReason;
		}
	});

	pi.on("ui_prompt_start", async () => {
		setPresenceState("blocked");
	});

	pi.on("ui_prompt_end", async (_event, ctx) => {
		setPresenceState(ctx?.isIdle?.() === false ? "busy" : "idle");
	});

	pi.on("session_info_changed", async (_event, ctx) => {
		// Identity, the address this session answers to, and the subscription all move
		// together, whether or not presence is on.
		const renamed: SessionIdentity = {
			alias: pi.getSessionName() ?? ctx.sessionManager.getSessionName(),
			sessionId: identity?.sessionId ?? ctx.sessionManager.getSessionId(),
		};
		identity = renamed;
		self = selfAddress(renamed);
		consumer = consumerId(renamed);
		resubscribe();
		if (presence && !presence.setOwner(consumer)) {
			report("msgq presence could not re-register after a rename", "warning");
		}
	});

	pi.on("session_shutdown", async () => {
		teardown();
	});
}
