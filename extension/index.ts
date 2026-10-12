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
	parseTopics,
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
		const plan = planDelivery(record, policy, self, busy);

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

	function onEvent(address: string, event: BusEvent, subscription: Subscription): void {
		const decoded = decodeRecord(event.payload);
		if (!decoded.ok) {
			// The envelope cannot be trusted, so it is never acknowledged.
			skipped += 1;
			report(`msgq skipped a record on ${address}: ${decoded.reason}`, "warning");
			return;
		}
		deliver(decoded.record, address, event.offset, subscription);
	}

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
		addresses = subscriptionAddresses(identity, extra);

		// Bus.
		const dbPath = resolveDbPath(process.env, homedir());
		try {
			const opened = new Bus(dbPath);
			bus = opened;
			subscriptions = addresses.map((address) => {
				const subscription = opened.subscribe(
					address,
					consumer,
					(event) => onEvent(address, event, subscription),
					(error) => disable(`bus subscription failed on ${address}: ${String(error)}`),
				);
				return subscription;
			});
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
		if (!presence) return;
		const renamed: SessionIdentity = {
			alias: pi.getSessionName() ?? ctx.sessionManager.getSessionName(),
			sessionId: identity?.sessionId ?? ctx.sessionManager.getSessionId(),
		};
		identity = renamed;
		if (!presence.setOwner(consumerId(renamed))) {
			report("msgq presence could not re-register after a rename", "warning");
		}
	});

	pi.on("session_shutdown", async () => {
		teardown();
	});
}
