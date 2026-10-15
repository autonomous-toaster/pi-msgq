// The human's command surface: /peers, /peer, /reply, /companions.
//
// Thin by design. Each command resolves what it was asked for, plans the same
// command the model's tools plan, and runs it — so the extension still writes
// nothing to the bus itself, and the wire stays in one place.

import type { ExtensionAPI, ExtensionCommandContext } from "@earendil-works/pi-coding-agent";

import {
	formatCompanions,
	formatIdentity,
	formatRoster,
	type IdentityFacts,
	parseCompanionList,
	parseSessions,
	type RosterEntry,
	resolveCommandAddress,
	rosterLabel,
	SELF_ALIAS,
	splitPeerArguments,
} from "./commands-text.ts";
import { runPlanned } from "./run.ts";
import { isError, planPeerAction } from "./tools.ts";

/** Everything a command needs to know about the session it speaks for. */
export interface CommandDeps {
	/** What this session is, and what it listens to. */
	identity(): IdentityFacts;
	/** The msgq binary, and the companion command. */
	msgqBin(): string;
	companionBin(): string;
	/** The message this session received most recently, for replies. */
	lastInbound(): { from: string; offset: number } | undefined;
	/** A reply went out for the message at this offset, so nothing is owed for it. */
	answered(offset: number): void;
}

/** A command needs a dialog, and this session cannot show one. */
const NEEDS_DIALOG = "choosing and composing need an interactive session: no dialog can be shown here";

/**
 * Say something to a person.
 *
 * This deliberately uses the command's own context rather than the extension's: a command
 * still has one after the extension has torn itself down, and "who am I on the bus" is a
 * question a human asks precisely when the bus is broken. With no UI at all — a script
 * driving the command — the only place left to report is stderr.
 */
function say(
	ctx: ExtensionCommandContext,
	message: string,
	level: "info" | "warning" | "error" = "info",
): void {
	if (ctx.hasUI) ctx.ui.notify(message, level);
	else process.stderr.write(`msgq: ${message}\n`);
}

/** Read the roster, or undefined when the bus cannot be read at all. */
async function readRoster(
	pi: ExtensionAPI,
	ctx: ExtensionCommandContext,
	deps: CommandDeps,
): Promise<RosterEntry[] | undefined> {
	const plan = { command: deps.msgqBin(), args: ["sessions", "--json"] };
	const result = await runPlanned(pi, plan, { binEnv: "MSGQ_BIN" });
	if (!result.ok) {
		say(ctx, `could not read the bus: ${result.text}`, "warning");
		return undefined;
	}
	return parseSessions(result.text);
}

/** The reachable participants other than this session. */
function others(roster: RosterEntry[], self: string): RosterEntry[] {
	return roster.filter((entry) => entry.address !== self);
}

/** Offer the peers and return the one chosen, or undefined. Silent when cancelled. */
async function choosePeer(
	ctx: ExtensionCommandContext,
	deps: CommandDeps,
	roster: RosterEntry[],
): Promise<string | undefined> {
	const peers = others(roster, deps.identity().self);
	if (peers.length === 0) {
		say(ctx, "no other participant is reachable on the bus");
		return undefined;
	}
	if (!ctx.hasUI) {
		say(ctx, NEEDS_DIALOG, "warning");
		return undefined;
	}
	const labels = peers.map(rosterLabel);
	const chosen = await ctx.ui.select("Send to a peer", labels);
	if (chosen === undefined) return undefined;
	const index = labels.indexOf(chosen);
	return index >= 0 ? peers[index].address : undefined;
}

/** Compose a message. Undefined when the editor was abandoned or left empty. */
async function compose(
	ctx: ExtensionCommandContext,
	deps: CommandDeps,
	title: string,
): Promise<string | undefined> {
	if (!ctx.hasUI) {
		say(ctx, NEEDS_DIALOG, "warning");
		return undefined;
	}
	const text = await ctx.ui.editor(title);
	const trimmed = text?.trim();
	return trimmed ? trimmed : undefined;
}

/** Run a planned send and report where it went and whether anybody is there. */
async function runSend(
	pi: ExtensionAPI,
	ctx: ExtensionCommandContext,
	deps: CommandDeps,
	plan: { command: string; args: string[] },
	checkRecipient: boolean,
): Promise<boolean> {
	const result = await runPlanned(pi, plan, { binEnv: "MSGQ_BIN" });
	if (!result.ok) {
		say(ctx, `could not send: ${result.text}`, "warning");
		return false;
	}
	const address = plan.args[1] ?? "";
	say(ctx, `sent to ${address}`);

	if (!checkRecipient) return true;
	// The message is durable, but a consumer that starts later begins at the head
	// of its stream, so "nobody is listening" is worth saying out loud.
	const roster = await readRoster(pi, ctx, deps);
	if (roster && !roster.some((entry) => entry.address === address)) {
		say(ctx, `nothing is listening at ${address} right now`, "warning");
	}
	return true;
}

/**
 * The planner context for a send. `reportTo` is deliberately absent: that fallback
 * is for a companion that must report without being told, not for a human, who
 * should be asked which peer they meant.
 */
function peerContext(deps: CommandDeps) {
	return {
		msgqBin: deps.msgqBin(),
		self: deps.identity().self,
		reportTo: undefined,
		lastInbound: undefined,
	};
}

/** Publish to an address, as this session. */
async function sendTo(
	pi: ExtensionAPI,
	ctx: ExtensionCommandContext,
	deps: CommandDeps,
	address: string,
	message: string,
): Promise<void> {
	const plan = planPeerAction({ action: "send", to: address, message }, peerContext(deps));
	if (isError(plan)) {
		say(ctx, plan.error, "warning");
		return;
	}
	await runSend(pi, ctx, deps, plan, true);
}

/** Answer the message this session received most recently. */
async function sendReply(
	pi: ExtensionAPI,
	ctx: ExtensionCommandContext,
	deps: CommandDeps,
	message: string,
): Promise<void> {
	const inbound = deps.lastInbound();
	if (!inbound) return;
	const self = deps.identity().self;
	const plan = planPeerAction(
		{ action: "reply", message },
		{ msgqBin: deps.msgqBin(), self, reportTo: undefined, lastInbound: inbound },
	);
	if (isError(plan)) {
		say(ctx, plan.error, "warning");
		return;
	}
	// The recipient just wrote to us, so there is nobody to warn about.
	if (await runSend(pi, ctx, deps, plan, false)) {
		// The question this session owed an answer to is settled.
		deps.answered(inbound.offset);
	}
}

/** Register the four commands. Called once, as the extension is loaded. */
export function registerCommands(pi: ExtensionAPI, deps: CommandDeps): void {
	pi.registerCommand("peers", {
		description:
			"Who is reachable on the message bus, and send one of them a message. " +
			"'/peers me' reports this session's own bus identity instead.",
		handler: async (args, ctx) => {
			if (args.trim() === SELF_ALIAS) {
				say(ctx, formatIdentity(deps.identity()));
				return;
			}
			const roster = await readRoster(pi, ctx, deps);
			if (!roster) return;
			if (!ctx.hasUI) {
				// The roster itself needs no dialog, so it is worth showing even here.
				const listing = formatRoster(roster, deps.identity().self);
				say(ctx, `${listing}\n${NEEDS_DIALOG}`, "warning");
				return;
			}
			const address = await choosePeer(ctx, deps, roster);
			if (!address) return;
			const message = await compose(ctx, deps, `Message to ${address}`);
			if (!message) {
				say(ctx, "nothing sent");
				return;
			}
			await sendTo(pi, ctx, deps, address, message);
		},
	});

	pi.registerCommand("peer", {
		description:
			"Send a message to a peer: /peer <address> [message]. 'me' is this session, a bare " +
			"name is session/<name>, no address asks which peer, and no message opens the editor. " +
			"Quote the message to give it with no address: /peer \"look at this\".",
		handler: async (args, ctx) => {
			const parsed = splitPeerArguments(args);
			let address = "";
			if (parsed.address) {
				const resolved = resolveCommandAddress(parsed.address, deps.identity().self);
				if (!resolved.ok) {
					say(ctx, resolved.reason, "warning");
					return;
				}
				address = resolved.address;
			} else {
				const roster = await readRoster(pi, ctx, deps);
				if (!roster) return;
				const chosen = await choosePeer(ctx, deps, roster);
				if (!chosen) return;
				address = chosen;
			}

			const message = parsed.message ?? (await compose(ctx, deps, `Message to ${address}`));
			if (!message) {
				say(ctx, "nothing sent");
				return;
			}
			await sendTo(pi, ctx, deps, address, message);
		},
	});

	pi.registerCommand("reply", {
		description: "Answer the message you most recently received: /reply [message]",
		handler: async (args, ctx) => {
			const inbound = deps.lastInbound();
			if (!inbound) {
				say(ctx, "nothing to reply to: no message has been received yet", "warning");
				return;
			}
			const message = args.trim() || (await compose(ctx, deps, `Reply to ${inbound.from}`));
			if (!message) {
				say(ctx, "nothing sent");
				return;
			}
			await sendReply(pi, ctx, deps, message);
		},
	});

	pi.registerCommand("companions", {
		description:
			"The companions running in terminal tabs: their status and working directory, " +
			"which the bus roster does not carry.",
		handler: async (_args, ctx) => {
			const plan = { command: deps.companionBin(), args: ["list", "--json"] };
			const result = await runPlanned(pi, plan, { binEnv: "MSGQ_COMPANION" });
			if (!result.ok) {
				say(ctx, `could not list companions: ${result.text}`, "warning");
				return;
			}
			say(ctx, formatCompanions(parseCompanionList(result.text)));
		},
	});
}
