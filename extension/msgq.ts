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
	HonkerBus,
	type Bus,
	type BusEvent,
	type Subscription,
} from "./src/consumer.ts";
import { PgBus } from "./src/pg-bus.ts";
import {
	ConfigError,
	consumerId,
	parseCardPaths,
	parseContext,
	parsePolicy,
	parsePresence,
	parseTopics,
	parseAllowList,
	parseTtlSeconds,
	type CardPathMode,
	type ContextMode,
	type Policy,
	recordId,
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
import {
	answersConsumedAsk,
	Coalescer,
	formatBatch,
	formatCommandNote,
	formatMessage,
	formatOwed,
	planDelivery,
	type ReferencedRecord,
	wantsReply,
} from "./src/delivery.ts";
import { type CommandDeps, registerCommands } from "./src/commands.ts";
import { openWithClient, type OpenOutcome } from "./src/open.ts";
import { clientPublisher, clientPublishRunner, type Publisher } from "./src/publish.ts";
import {
	clientRunner,
	emptySessionTrust,
	formatTrustReport,
	parseTrustRequest,
	sessionEnv,
	type SessionTrust,
	verdictLine,
	verifyRecord,
} from "./src/trust.ts";
import {
	type Card,
	type CardOverrides,
	CARD_MAX_BYTES,
	cardBytes,
	cardSummary,
	cardWithinLimit,
	mergeCard,
	parseCardOverrides,
	parseDerivedCard,
	sameCard,
} from "./src/card.ts";
import { formatIdentity, type IdentityFacts } from "./src/commands-text.ts";
import { runPlanned } from "./src/run.ts";
import { Presence, PRESENCE_TTL_S } from "./src/presence.ts";
import type { PresenceState } from "./src/presence-text.ts";
import { decodeRecord, openRecord, type MsgqRecord, recordBody } from "./src/record.ts";
import { describeKey, loadKey, type SealingKey } from "./src/seal.ts";
import {
	askWaitSeconds,
	interpretAsk,
	isError,
	MAX_ASK_TIMEOUT_S,
	PLAIN_LIST,
	planPeerAction,
	shouldRetryPlainList,
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
	pi.registerFlag("msgq-allow", {
		description: "Comma-separated senders allowed to start a turn under 'honor'",
		type: "string",
	});
	pi.registerFlag("msgq-context", {
		description: "Derive peer locator context: 'auto' (default) or 'off'",
		type: "string",
	});
	pi.registerFlag("msgq-card-paths", {
		description: "Locator path mode: 'relative' (default), 'full', or 'none'",
		type: "string",
	});

	pi.registerMessageRenderer(MESSAGE_TYPE, renderMsgq);

	let bus: Bus | null = null;
	let subscriptions: Subscription[] = [];
	let coalescer = new Coalescer();
	// What this conversation believes, as opposed to what the keyring on disk believes.
	let sessionTrust: SessionTrust = emptySessionTrust();
	let context: ExtensionContext | undefined;
	let presence: Presence | undefined;
	let renewTimer: ReturnType<typeof setInterval> | undefined;
	let identity: SessionIdentity | undefined;
	let self = "";
	let consumer = "";
	let policy: Policy = "display";
	/** The key this session seals and opens bodies with, when a passphrase is configured. */
/** Half of the default one-hour lease, in milliseconds. */
	const HALF_LEASE_MS = 30 * 60 * 1000;
	let leaseTimer: ReturnType<typeof setInterval> | undefined;
	let sealingKey: SealingKey | undefined;

	/** Renew once now, then at half the lease, so the deadline never passes unnoticed. */
	function renewLease(shared: PgBus): void {
		const attempt = () => {
			void shared.renew().catch((error: unknown) => {
				report(`msgq could not renew its credential: ${String(error)}`, "warning");
			});
		};
		attempt();
		if (leaseTimer) clearInterval(leaseTimer);
		leaseTimer = setInterval(attempt, HALF_LEASE_MS);
		// A timer must not be the reason the process stays alive.
		leaseTimer.unref?.();
	}
	let addresses: string[] = [];
	let skipped = 0;
	/** Senders allowed to start a turn under the honour policy. Empty means no allowlist. */
	let allowed: string[] = [];
	/** Whether presence is configured on, as opposed to actually held. */
	let presenceConfigured = false;
	/** Whether the session currently holds its presence lease. */
	let presenceHeld = false;
	/** Whether locator context is derived, and how much path it carries. */
	let contextMode: ContextMode = "auto";
	let cardPaths: CardPathMode = "relative";
	/** The last derived locator, the per-field overrides, and what presence currently carries. */
	let derivedCard: Card = {};
	let cardOverrides: CardOverrides = {};
	let currentCard: Card | undefined;
	let currentCwd = "";
	/** Guards overlapping `msgq card` runs during a slow git call. */
	let refreshingCard = false;
	/** A refresh requested while one was already running. */
	let cardRefreshQueued = false;
	/** Where the bus is. Resolved once: it does not depend on the session. */
	const busPath = resolveDbPath(process.env, homedir());
	/** Extra addresses beyond this session's own, kept so a rename can rebuild the set. */
	let extraTopics: string[] = [];
	/** The most recently received message, so a reply can target it. */
	let lastInbound: { from: string; offset: number } | undefined;
	/**
	 * The peers this session is currently holding its turn open for.
	 *
	 * A wait holds the one execution slot the session has, so a question arriving from the peer
	 * being waited on can never be answered while it lasts. Knowing which peer that is, is what
	 * turns a mutual wait into a sentence the model can act on.
	 */
	let waitedOn = new Map<string, { controller: AbortController; abandoned: boolean }>();
	/**
	 * Peers whose answer was already returned inside a tool result during this turn.
	 *
	 * The record is still on the stream afterwards, so this is what keeps the answer from being
	 * delivered a second time. Cleared when a turn starts: it can suppress one turn's worth of
	 * duplication, and nothing later.
	 */
	let answeredPeers: string[] = [];
	/**
	 * The question this session was asked and may still owe an answer to.
	 *
	 * One slot on purpose: the delivery path remembers as little as it can, so a session asked two
	 * questions at once is nudged about the later one rather than both.
	 */
	let owed: { offset: number; from: string; asked: boolean; answered: boolean } | undefined;
	/** How to reach the two binaries the tools delegate to. */
	/** Acknowledged records awaiting a run, so start and completion can be reported. */
	let inFlight: Array<{ offset: number; to: string; phases: AckPhase[] }> = [];
	/** The stop reason of the last assistant message in the current run. */
	let lastStopReason: string | undefined;

	function report(message: string, level: "info" | "warning" | "error" = "info"): void {
		if (context?.hasUI) context.ui.notify(message, level);
	}

	/**
	 * One signal that aborts when either the caller's does or the extension decides to give up.
	 *
	 * A waiting ask is abandoned when the peer asks this session something, and cancelled when the
	 * human interrupts — the two reasons are indistinguishable to the process, so the caller keeps
	 * a flag to tell them apart.
	 */
	function linkSignal(controller: AbortController, signal: AbortSignal | undefined): AbortSignal {
		if (!signal) return controller.signal;
		if (signal.aborted) controller.abort();
		else signal.addEventListener("abort", () => controller.abort(), { once: true });
		return controller.signal;
	}

	function setStatus(text: string | undefined): void {
		if (context?.hasUI) context.ui.setStatus(STATUS_KEY, text);
	}

	/** Why a presence write did not land, in words: the bus error, or the usual cause. */
	function presenceWhy(): string {
		const failure = presence?.error;
		if (failure === undefined) return "the lease is held elsewhere";
		if (failure instanceof Error) return failure.message || "the write failed without a reason";
		return String(failure);
	}

	/** Derive the locator, or undefined when context is off or derivation fails. */
	async function deriveCard(cwd: string): Promise<Card | undefined> {
		if (contextMode === "off") return undefined;
		try {
			const result = await pi.exec(
				msgqBin,
				["card", "--cwd", cwd, "--paths", cardPaths],
				{ cwd, timeout: 3000 },
			);
			if (result.code !== 0) return undefined;
			derivedCard = parseDerivedCard(JSON.parse(result.stdout.trim()));
			const card = mergeCard(derivedCard, cardOverrides);
			if (Object.keys(card).length === 0) return undefined;
			if (!cardWithinLimit(card)) {
				report(
					`msgq locator card is ${cardBytes(card)} bytes; omitting it (limit ${CARD_MAX_BYTES})`,
					"warning",
				);
				return undefined;
			}
			return card;
		} catch {
			return undefined;
		}
	}

	/** Re-derive the locator and publish it, only when it actually changed. */
	async function refreshCard(cwd: string, force = false): Promise<void> {
		if (contextMode === "off" || !presence) return;
		if (refreshingCard) {
			cardRefreshQueued = true;
			return;
		}
		refreshingCard = true;
		try {
			const next = await deriveCard(cwd);
			if (!force && sameCard(next, currentCard)) return;
			currentCard = next;
			presenceHeld = await presence.setCard(next);
			if (!presenceHeld) {
				report(`msgq presence could not publish its locator card: ${presenceWhy()}`, "warning");
			}
		} finally {
			refreshingCard = false;
			if (cardRefreshQueued) {
				cardRefreshQueued = false;
				void refreshCard(currentCwd, true);
			}
		}
	}

	/**
	 * Report a second participant publishing presence at this session's address.
	 *
	 * The wire cannot prevent it — `from` is a claim, and two sessions may legitimately share an
	 * alias — but it must not be silent, so the roster is read once and the collision is named. A
	 * roster that cannot be read changes nothing.
	 */
	async function reportPresenceCollision(): Promise<void> {
		const address = self;
		if (!address) return;
		try {
			const result = await pi.exec(msgqBin, ["--db", busPath, "sessions", "--json"], {});
			if (result.code !== 0) return;
			const count = result.stdout
				.split("\n")
				.filter(Boolean)
				.map((line) => JSON.parse(line) as { address?: string })
				.filter((entry) => entry.address === address).length;
			if (count > 1) {
				report(
					`msgq: ${count} participants are publishing presence at ${address}; the roster lists all of them`,
					"warning",
				);
			}
		} catch {
			// Reading the roster is a diagnostic here: failing to do it changes nothing.
		}
	}

	/** Move presence to a new state, reporting a lost lease without failing. */
	function setPresenceState(state: PresenceState): void {
		if (!presence) return;
		void presence.setState(state).then((held) => {
			presenceHeld = held;
			if (!held) {
				report(`msgq presence is not registered (${state}): ${presenceWhy()}`, "warning");
			}
		});
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
		if (!bus) return;
		publishSafely(
			() => signedPublish.publish(options.to, buildAck({ from: self, ...options })),
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
		// A retraction is a write: it is started here and settles on its own, because teardown is
		// not a place to wait for the network.
		void presence?.stop();
		presence = undefined;
		presenceHeld = false;
		derivedCard = {};
		cardOverrides = {};
		currentCard = undefined;
		currentCwd = "";
		refreshingCard = false;
		cardRefreshQueued = false;
		identity = undefined;
		for (const waiting of waitedOn.values()) waiting.controller.abort();
		waitedOn = new Map();
		answeredPeers = [];
		inFlight = [];
		lastStopReason = undefined;
		lastInbound = undefined;
		owed = undefined;
		for (const subscription of subscriptions) subscription.close();
		subscriptions = [];
		coalescer = new Coalescer();
		// Before the handle goes: this session's readers are closed, so the log can be folded
		// back and emptied instead of sitting at its high-water mark for whoever is left.
		bus?.truncateWal();
		void bus?.close();
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

	/**
	 * A reply went out, so whatever question was owed an answer is settled.
	 *
	 * The reply paths are this extension's own — the peers tool and the `/reply` command both build
	 * `--reply-to` — so it can see when one is sent rather than guessing.
	 */
	function noteAnswered(offset: number | undefined): void {
		if (owed && offset !== undefined && owed.offset === offset) owed.answered = true;
	}

	function itemOf(record: MsgqRecord, address: string, verdict?: MsgqItem["verdict"]): MsgqItem {
		return { from: record.from, address, type: record.type, body: recordBody(record), verdict };
	}

	/**
	 * Resolve the record a delivered message refers to, at delivery time.
	 *
	 * Nothing is remembered between records. A reference is an offset in the stream named
	 * by the referencing record's own sender, so a reply to a question asked from a shell,
	 * or before this session started, resolves exactly like one asked a moment ago. A
	 * reference that cannot be read still names its id, so the reader can tell that
	 * something is being answered.
	 */
	function referenceFor(record: MsgqRecord): ReferencedRecord | undefined {
		const offset = record.reply_to;
		if (typeof offset !== "number") return undefined;
		const id = recordId(record.from, offset);
		const event = bus?.readOne(record.from, offset);
		if (!event) return { id };
		const decoded = decodeRecord(event.payload);
		return decoded.ok ? { id, record: decoded.record } : { id };
	}

	/**
	 * Deliver a command as input, so the session runs it rather than reading about it.
	 *
	 * The note goes first, because a command must be the whole text and cannot carry its sender
	 * inside itself; a pending custom message is flushed before a prompt is sent, so the two land in
	 * that order. Dispatching is what a typed command does, so an unknown one passes through as
	 * ordinary text instead of failing, and the session runs whatever a person would have run.
	 */
	function deliverCommand(
		record: MsgqRecord,
		address: string,
		offset: number,
		subscription: Subscription,
		busy: boolean,
	): void {
		// A command is opened like anything else before it is handed to the session: dispatching a
		// sealed body would type ciphertext into the transcript and run it as a prompt.
		const opened = openThroughClient(record);
		if (opened.kind !== "opened") {
			const why = opened.kind === "elsewhere" ? `sealed to ${opened.sealedTo.join(", ")}` : opened.reason;
			report(`msgq cannot read a command from ${record.from}: ${why}`, "error");
			subscription.save(offset);
			return;
		}
		const body = recordBody(opened.record as MsgqRecord);
		const phases = isAckRecord(record) ? undefined : parseAckRequest(record.meta);
		if (phases) {
			publishAck({ to: record.from, replyTo: offset, phase: "received", delivery: "turn" });
			trackInFlight(record, offset, phases);
		}

		try {
			pi.sendMessage(
				{
					customType: MESSAGE_TYPE,
					content: formatCommandNote(record, address, referenceFor(record)),
					display: true,
					details: {
						items: [itemOf(record, address, verdictOf(record))],
					} satisfies MsgqDetails,
				},
				{ triggerTurn: false },
			);
			void pi
				.sendUserMessage(body, {
					expandPromptTemplates: true,
					// A command arriving mid-run is queued rather than batched, like follow-up work.
					deliverAs: busy ? "followUp" : undefined,
				})
				.catch((error) => {
					report(`msgq could not run a command: ${String(error)}`, "warning");
				});
		} catch (error) {
			report(`msgq could not deliver a command: ${String(error)}`, "warning");
		}

		// Responsibility is taken: the command has been handed over, whatever it then does.
		subscription.save(offset);
	}

	function deliver(
		record: MsgqRecord,
		address: string,
		offset: number,
		subscription: Subscription,
	): void {
		// The bytes the signature covers are the bytes on the bus, so the verdict is reached before the
		// body is opened. Verifying afterwards would compare a signature against plaintext it never
		// saw, and every sealed record would read as refused.
		const stored = record;

		// An unreadable body is an error, not content. Opening happens before `type` is consulted, so
		// no policy can turn an unopened body into a turn and nothing claims it was received.
		const opened = openThroughClient(record);
		if (opened.kind === "elsewhere") {
			// Sealed for somebody else: a fact about the record, and not this session's to read.
			report(
				`msgq: a record from ${record.from} is sealed to ${opened.sealedTo.join(", ")}`,
				"info",
			);
			subscription.save(offset);
			return;
		}
		if (opened.kind === "cannot") {
			report(`msgq cannot read a record from ${record.from}: ${opened.reason}`, "error");
			subscription.save(offset);
			return;
		}
		record = opened.record as MsgqRecord;

		// An acknowledgement is never itself acknowledged.
		const phases = isAckRecord(record) ? undefined : parseAckRequest(record.meta);
		const busy = context ? !context.isIdle() : false;
		const plan = planDelivery(record, policy, self, busy, allowed);
		const reference = referenceFor(record);

		// The peer being waited on is asking something. Answering needs the turn the wait is
		// holding, so the wait has to end for either side to make progress: this is the shape
		// two sessions asking each other would otherwise sit in until both windows close.
		const waiting = isAckRecord(record) ? undefined : waitedOn.get(record.from);
		if (waiting && wantsReply(record)) {
			waiting.abandoned = true;
			waiting.controller.abort();
		}

		// A wait already returned this answer inside the tool result. The record it was carried
		// in is still on the stream, and delivering it again is the whole answer twice.
		if (answersConsumedAsk(record, self, answeredPeers, reference)) {
			report(`msgq: the answer from ${record.from} was already returned by the ask that waited for it`);
			subscription.save(offset);
			return;
		}

		// A question the session will actually be asked starts an obligation it may still owe. A
		// display-only record is not one: nothing was asked of the agent, and asking it to answer
		// would start the very turn the policy refused.
		if ((plan === "turn" || plan === "buffer") && wantsReply(record) && record.from !== self) {
			owed = { offset, from: record.from, asked: false, answered: false };
		}

		if (plan === "buffer") {
			// Responsibility is not taken yet: a crash here must redeliver.
			coalescer.add(
				record,
				address,
				offset,
				() => subscription.save(offset),
				reference,
				verdictOf(stored),
			);
			return;
		}

		if (plan === "dispatch") {
			deliverCommand(record, address, offset, subscription, busy);
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

		const verdict = verdictOf(stored);

		try {
			pi.sendMessage(
				{
					customType: MESSAGE_TYPE,
					// The model reads a plain line; the transcript colours it, which is why the verdict
					// travels in the details as well as in the content.
					content: formatMessage(record, address, reference, verdictLine(verdict)),
					display: true,
					details: {
						items: [itemOf(record, address, verdict)],
					} satisfies MsgqDetails,
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
				(dropped) =>
					report(
						`msgq is ${dropped} record(s) behind the retention window on ${address}; ` +
							"continuing from the oldest retained record",
						"warning",
					),
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

	// The agent-facing surface: one tool that delegates, so the wire lives in the CLI.
	const msgqBin = process.env.MSGQ_BIN?.trim() || "msgq";

	/**
	 * How this session writes its own records.
	 *
	 * Presence and acknowledgements are records like any other, so they leave through the client and
	 * carry its signature. A bus where the client's own progress was unsigned would need a reader
	 * rule saying so, and a rule like that is a hole the moment somebody writes a record that looks
	 * like progress.
	 */
	const signedPublish: Publisher = clientPublisher(
		clientPublishRunner(msgqBin, { db: busPath }),
	);

	/**
	 * How this session reads a sealed body.
	 *
	 * Through the client, for the same reason a verdict is: one implementation of the encoding, and the
	 * extension keeps no copy of it. A plaintext record costs nothing.
	 */
	function openThroughClient(record: MsgqRecord): OpenOutcome {
		return openWithClient(
			(candidate) =>
				clientRunner(msgqBin)(
					["--db", busPath, "open", "--json"],
					`${JSON.stringify(candidate)}\n`,
					{},
				),
			record,
		);
	}

	/** The client, asked what one record is worth. */
	const trustRunner = clientRunner(msgqBin);

	/**
	 * The verdict line a delivered record carries, and the tally of who has not been pinned.
	 *
	 * The answer comes from the client, never from this module: a second implementation of the
	 * signature would be a second answer to the same question.
	 */
	function verdictOf(record: unknown): Verdict {
		const verdict = verifyRecord(trustRunner, record, sessionTrust);
		if (verdict.state === "unverified") sessionTrust.unverified += 1;
		return verdict;
	}

	/** Run a trust verb through the client, with this session's layer in its environment. */
	function runTrust(args: string[]): string {
		return clientRunner(msgqBin)(args, undefined, sessionEnv(sessionTrust));
	}

	function toolText(content: string, details?: unknown) {
		return { content: [{ type: "text" as const, text: content }], details };
	}

	/**
	 * What this session is, as one report.
	 *
	 * Composed once and read by both audiences — the model through the peers tool, and a human
	 * through `/peers me` — so the two answers cannot drift apart.
	 */
	function identityFacts(): IdentityFacts {
		return {
			self,
			bus: busPath,
			addresses,
			policy,
			presence: presenceConfigured ? (presenceHeld ? "registered" : "lost") : "off",
			allowed,
			card: currentCard,
			write: writeState(),
			encryption: describeKey(sealingKey),
		};
	}

	/**
	 * Whether this session can write, as far as it can tell.
	 *
	 * A refused write is not an idle session: the difference is exactly what an operator needs when
	 * a session goes quiet, so it is reported by name rather than inferred from silence.
	 */
	function writeState(): string | undefined {
		const failure = presence?.error;
		if (failure === undefined) {
			return presenceHeld ? "writable" : undefined;
		}
		const text = failure instanceof Error ? failure.message : String(failure);
		if (/credential|scope|refus|permission|42501|28P01|28000/i.test(text)) {
			return `read-only (${text})`;
		}
		return `write failed (${text})`;
	}

	async function handleCardAction(value: unknown): Promise<ReturnType<typeof toolText>> {
		if (contextMode === "off") {
			return toolText("locator context is disabled; nothing was changed");
		}
		if (value === undefined) {
			return toolText(
				currentCard && Object.keys(currentCard).length > 0
					? JSON.stringify(currentCard, null, 2)
					: "no locator card is set",
			);
		}
		const parsed = parseCardOverrides(value);
		if (!parsed.ok) return toolText(parsed.error);
		if (!currentCwd) currentCwd = context?.cwd ?? process.cwd();
		cardOverrides = { ...cardOverrides, ...parsed.overrides };
		await refreshCard(currentCwd, true);
		return toolText(
			currentCard && Object.keys(currentCard).length > 0
				? `card updated\n${JSON.stringify(currentCard, null, 2)}`
				: "card cleared",
		);
	}

	const peersTool = {
		name: "peers",
		label: "peers",
		description:
			"Work with other agents on the message bus. 'list' the addresses that are reachable, " +
			"'send' a message to one, 'ask' a question (by default the answer arrives later as a " +
			"message; add wait to have it returned in this turn), or 'reply' to the " +
			"message you most recently received. 'card' reports or changes this session's locator " +
			"(project, branch, worktree, path); omit the card to read it. To have a peer *run* " +
			"something rather than be told about it, send a command: the body is the command line, " +
			"and its own arguments are where you ask for the answer. Use it to hand work to another " +
			"session on the bus and to read what comes back.",
		parameters: Type.Object({
			action: Type.Union(
				[
					Type.Literal("list"),
					Type.Literal("send"),
					Type.Literal("ask"),
					Type.Literal("reply"),
					Type.Literal("card"),
				],
				{ description: "what to do" },
			),
			to: Type.Optional(
				Type.String({ description: "the peer's address, for example session/reviewer" }),
			),
			message: Type.Optional(Type.String({ description: "what to say or ask" })),
			command: Type.Optional(
				Type.Boolean({
					description:
						"send a command for the peer to run instead of a message for it to read: the body must be the command line and nothing else",
				}),
			),
			wait: Type.Optional(
				Type.Boolean({
					description:
						"for 'ask': hold this turn open for the answer instead of being woken by it. Bounded, because two sessions waiting on each other cannot answer",
				}),
			),
			timeoutSeconds: Type.Optional(
				Type.Number({
					description: `for 'ask' with wait: how long to wait, in seconds, at most ${MAX_ASK_TIMEOUT_S}`,
				}),
			),
			card: Type.Optional(
				Type.Object(
					{
						project: Type.Optional(Type.Union([Type.String(), Type.Null()])),
						branch: Type.Optional(Type.Union([Type.String(), Type.Null()])),
						worktree: Type.Optional(Type.Union([Type.String(), Type.Null()])),
						path: Type.Optional(Type.Union([Type.String(), Type.Null()])),
					},
					{
						description:
							"locator overrides for the 'card' action; null clears one and returns it to the derived value",
						additionalProperties: false,
					},
				),
			),
		}),
		async execute(
			_toolCallId: string,
			params: {
				action: "list" | "send" | "ask" | "reply" | "card";
				to?: string;
				message?: string;
				timeoutSeconds?: number;
				wait?: boolean;
				command?: boolean;
				card?: unknown;
			},
			signal: AbortSignal | undefined,
		) {
			if (params.action === "card") return handleCardAction(params.card);
			const plan = planPeerAction(params, {
				msgqBin,
				self,
				lastInbound,
				// The model chose not to wait, so ask to be told when the peer's run finishes.
				expectCompletion: true,
			});
			if (isError(plan)) return toolText(plan.error);

			// Only a waiting ask holds the turn open, so only it can need abandoning or leave an
			// answer behind on the stream.
			const waiting = params.action === "ask" && params.wait === true;
			const to = params.to ?? "";
			const entry = waiting ? { controller: new AbortController(), abandoned: false } : undefined;
			const seconds = waiting ? askWaitSeconds(params.timeoutSeconds) : 0;
			const timeoutMs = waiting ? seconds * 1000 + 30_000 : undefined;
			if (entry) waitedOn.set(to, entry);
			let report: { ok: boolean; text: string };
			try {
				report = await runPlanned(pi, plan, {
					signal: entry ? linkSignal(entry.controller, signal) : signal,
					timeoutMs,
					binEnv: "MSGQ_BIN",
				});
			} finally {
				if (entry) waitedOn.delete(to);
			}
			if (entry?.abandoned) {
				// Its question is buffered behind this turn and arrives as soon as the turn ends.
				// The question this session sent is published and unanswered, not lost.
				return toolText(
					`${to} is asking you a question and cannot answer until you do, so this session stopped waiting. Its question arrives as your next message: answer it with peers({action:"reply"}), then ask again — the question you sent is not lost.`,
				);
			}
			if (waiting) {
				const ask = interpretAsk(report, to);
				// Consumed already: the record must not be delivered a second time later in this turn.
				if (ask.answered) answeredPeers.push(to);
				report = { ok: report.ok, text: ask.text };
			}
			// Older CLIs do not know --wide. The locator is an enhancement, so fall back to the
			// plain roster rather than failing the list action — but only when the CLI rejected
			// the flag, not when it could not read the bus.
			if (params.action === "list" && shouldRetryPlainList(plan, report)) {
				report = await runPlanned(
					pi,
					{ command: plan.command, args: [...PLAIN_LIST] },
					{ signal, timeoutMs, binEnv: "MSGQ_BIN" },
				);
			}
			// A reply went out, so the question this session owed an answer to is settled.
			if (params.action === "reply" && report.ok) noteAnswered(lastInbound?.offset);
			// "Who is out there" and "which of them is me" are one question, so the roster
			// answers both: this session's own identity first, then the reachable peers.
			if (params.action !== "list") return toolText(report.text);
			return toolText(`${formatIdentity(identityFacts())}\n\n${report.text}`);
		},
	};
	pi.registerTool(peersTool);


	// The human's surface: the same planners, run from the prompt instead of by a model.
	// Registered here, as the extension loads, so the commands exist from the start.
	const commandDeps: CommandDeps = {
		identity: identityFacts,
		msgqBin: () => msgqBin,
		lastInbound: () => lastInbound,
		answered: noteAnswered,
		trust: () => ({ session: sessionTrust, run: runTrust }),
	};
	registerCommands(pi, commandDeps);

	pi.on("session_start", async (_event, ctx) => {
		teardown();
		context = ctx;
		skipped = 0;

		// Configuration.
		let extra: string[];
		let presenceTtl: number;
		try {
			const topicsFlag = pi.getFlag("msgq-topics");
			const policyFlag = pi.getFlag("msgq-policy");
			const presenceFlag = pi.getFlag("msgq-presence");
			const allowFlag = pi.getFlag("msgq-allow");
			const contextFlag = pi.getFlag("msgq-context");
			const cardPathsFlag = pi.getFlag("msgq-card-paths");
			extra = parseTopics(
				(typeof topicsFlag === "string" ? topicsFlag : undefined) ??
					process.env.MSGQ_TOPICS,
			);
			policy = parsePolicy(
				(typeof policyFlag === "string" ? policyFlag : undefined) ??
					process.env.MSGQ_POLICY,
			);
			presenceConfigured = parsePresence(
				(typeof presenceFlag === "string" ? presenceFlag : undefined) ??
					process.env.MSGQ_PRESENCE,
			);
			presenceTtl = parseTtlSeconds(process.env.MSGQ_PRESENCE_TTL, PRESENCE_TTL_S);
			allowed = parseAllowList(
				(typeof allowFlag === "string" ? allowFlag : undefined) ??
					process.env.MSGQ_ALLOW,
			);
			contextMode = parseContext(
				(typeof contextFlag === "string" ? contextFlag : undefined) ??
					process.env.MSGQ_CONTEXT,
			);
			cardPaths = parseCardPaths(
				(typeof cardPathsFlag === "string" ? cardPathsFlag : undefined) ??
					process.env.MSGQ_CARD_PATHS,
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
		currentCwd = ctx.cwd;

		// Keys. A broken configured source is reported and treated as absent, so reads refuse sealed
		// bodies instead of pretending the session can open them.
		try {
			sealingKey = loadKey();
		} catch (error) {
			sealingKey = undefined;
			report(`msgq cannot use the configured passphrase: ${String(error)}`, "warning");
		}

		// Bus. A connection string selects the shared engine; anything else is a file.
		try {
			if (busPath.includes("://")) {
				bus = await PgBus.open(busPath, subscriptionAddresses(identity, extraTopics));
				// A session may stay silent far longer than its lease. Renewing at start and again at
				// half the lease keeps a resumed session usable without minting a second credential.
				renewLease(bus);
			} else {
				bus = new HonkerBus(busPath);
			}
			resubscribe();
		} catch (error) {
			disable(`cannot open ${busPath}: ${String(error)}`);
			return;
		}

		// Presence: a scoped write exception, published as a record any client could publish. A failure
		// here is never fatal.
		if (presenceConfigured && bus) {
			currentCard = await deriveCard(currentCwd);
			// Presence goes out through the client too, so the bus holds no unsigned record this session
			// wrote: a forged presence is refused like any other forgery.
			presence = new Presence(signedPublish, {
				address: self,
				participant: identity.sessionId,
				alias: identity.alias,
				state: ctx.isIdle() ? "idle" : "busy",
				ttlS: presenceTtl,
				card: currentCard,
				scope: process.env.MSGQ_SCOPE?.trim() || undefined,
			});
			presenceHeld = await presence.start();
			if (!presenceHeld) {
				report(`msgq presence is not registered (${presenceWhy()})`, "warning");
			}
			void reportPresenceCollision();
			const renewMs = Math.max(1, Math.floor(presenceTtl / 3)) * 1000;
			let renewCount = 0;
			renewTimer = setInterval(() => {
				if (!presence) return;
				renewCount += 1;
				if (contextMode === "auto" && renewCount % 6 === 0) void refreshCard(currentCwd);
				const held = presenceHeld;
				// A presence write is best-effort and cannot throw: a full disk or a closed
				// handle must not take the session down with it.
				void presence.renew().then((nowHeld) => {
					presenceHeld = nowHeld;
					// Only the change is news. Reporting every failure would repeat the same line
					// each TTL, and a bus that came back would go unmentioned.
					if (held && !nowHeld) {
						report(`msgq presence was lost: ${presenceWhy()}`, "warning");
					} else if (!held && nowHeld) {
						report("msgq presence is registered", "info");
					}
				});
			}, renewMs);
			renewTimer.unref?.();
		}

		setStatus(`msgq: ${addresses.length} (${policy})`);
		report(`msgq: following ${addresses.join(", ")} as ${consumer} (${policy})`);
	});

	pi.on("agent_before_settle", async (_event: AgentBeforeSettleEvent) => {
		const entries: NonNullable<AgentBeforeSettleEventResult["entries"]> = [];

		if (!coalescer.empty) {
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
				items: batch.map((item) => itemOf(item.record, item.address, item.verdict)),
			};
			entries.push({
				type: "custom_message",
				customType: MESSAGE_TYPE,
				content: formatBatch(batch),
				display: true,
				details,
			});
		}

		// The reply convention is advisory, so a question can survive a whole run unanswered. Ask
		// for it once, as it is about to be forgotten, and say out loud that declining is allowed.
		if (owed && !owed.answered && !owed.asked) {
			owed.asked = true;
			const id = recordId(owed.from, owed.offset);
			report(`msgq: asked for the answer owed to ${owed.from}`);
			entries.push({
				type: "custom_message",
				customType: MESSAGE_TYPE,
				content: formatOwed({ from: owed.from, id }),
				display: true,
				details: {
					items: [{ from: owed.from, address: self, type: "note", body: `unanswered: ${id}` }],
				} satisfies MsgqDetails,
			});
		}

		if (entries.length === 0) return;
		const result: AgentBeforeSettleEventResult = { entries, continue: true };
		return result;
	});

	pi.on("input", async (_event, ctx) => {
		currentCwd = ctx.cwd;
		void refreshCard(currentCwd);
	});

	pi.on("agent_start", async (_event, ctx) => {
		lastStopReason = undefined;
		// Duplicate suppression lasts one turn: nothing a previous turn answered can be consumed now.
		answeredPeers = [];
		currentCwd = ctx.cwd;
		void refreshCard(currentCwd);
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
		if (presence) {
			presenceHeld = await presence.setOwner({ address: self, alias: renamed.alias });
			if (!presenceHeld) {
				report(`msgq presence could not re-register after a rename: ${presenceWhy()}`, "warning");
			}
		}
	});

	pi.on("session_shutdown", async () => {
		teardown();
	});
}
