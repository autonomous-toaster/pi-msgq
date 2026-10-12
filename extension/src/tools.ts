// Command planning for the agent-facing tools.
//
// Pure: the planners build the command line and decide what is missing. The
// extension runs the result, so everything here is unit-testable.

/** A command to run. */
export interface Command {
	command: string;
	args: string[];
}

/** Something the caller must fix before the command can be built. */
export interface PlanError {
	error: string;
}

export function isError(plan: Command | PlanError): plan is PlanError {
	return "error" in plan;
}

/** Arguments accepted by the peers tool. */
export interface PeerParams {
	action: "list" | "send" | "ask" | "reply";
	to?: string;
	message?: string;
	timeoutSeconds?: number;
}

/** What the peers planner needs to know about this session. */
export interface PeerContext {
	/** The msgq binary to invoke. */
	msgqBin: string;
	/** This session's own address. */
	self: string;
	/** Default recipient when a call does not name one. */
	reportTo?: string;
	/** The most recently received message, for replies. */
	lastInbound?: { from: string; offset: number };
}

/** Default window for ask, in seconds. */
export const DEFAULT_ASK_TIMEOUT_S = 600;

/**
 * Refuse to build a command without an identity. Sending as nobody produces a record
 * with an empty sender, and spawning with no parent produces a companion with no
 * trusted-sender allowlist, which is worse.
 */
const NO_IDENTITY = "this session has no bus address yet: the message bus did not start";

/** Plan a peers action. */
export function planPeerAction(params: PeerParams, context: PeerContext): Command | PlanError {
	const action = params.action as string;
	if (action === "list") {
		return { command: context.msgqBin, args: ["sessions"] }; // needs no identity
	}
	if (!context.self) return { error: NO_IDENTITY };

	if (action === "send") {
		const to = params.to ?? context.reportTo;
		if (!to) return { error: "no recipient: pass 'to', or configure a reporting address" };
		if (!params.message) return { error: "no message to send" };
		// Send as this session, not as the CLI's default sender: the peer's trusted
		// sender list, and its notion of who to answer, both depend on it.
		return {
			command: context.msgqBin,
			args: ["publish", to, params.message, "--type", "prompt", "--from", context.self],
		};
	}

	if (action === "ask") {
		const to = params.to ?? context.reportTo;
		if (!to) return { error: "no recipient: pass 'to', or configure a reporting address" };
		if (!params.message) return { error: "no question to ask" };
		const timeout = params.timeoutSeconds ?? DEFAULT_ASK_TIMEOUT_S;
		return {
			command: context.msgqBin,
			args: ["ask", to, params.message, "--timeout", String(timeout), "--from", context.self],
		};
	}

	if (action === "reply") {
		if (!params.message) return { error: "no reply to send" };
		const inbound = context.lastInbound;
		if (!inbound) return { error: "nothing to reply to: no message has been received yet" };
		return {
			command: context.msgqBin,
			args: [
				"publish",
				params.to ?? inbound.from,
				params.message,
				"--type",
				"prompt",
				"--reply-to",
				String(inbound.offset),
				"--from",
				context.self,
			],
		};
	}

	return { error: "unknown action: " + action };
}

/** Arguments accepted by the companions tool. */
export interface CompanionParams {
	action: "spawn" | "close";
	name?: string;
	cwd?: string;
}

/** What the companions planner needs to know. */
export interface CompanionContext {
	/** The companion command to invoke. */
	companionBin: string;
	/** This session's own address, which the companion reports to. */
	self: string;
}

/** Plan a companions action. */
export function planCompanionAction(
	params: CompanionParams,
	context: CompanionContext,
): Command | PlanError {
	const action = params.action as string;

	if (action === "spawn") {
		if (!params.name) return { error: "a companion needs a name" };
		// A companion with no parent would be spawned with no trusted-sender allowlist.
		if (!context.self) return { error: NO_IDENTITY };
		const args = ["spawn", params.name, "--parent", context.self, "--json"];
		if (params.cwd) args.push("--cwd", params.cwd);
		return { command: context.companionBin, args };
	}

	if (action === "close") {
		if (!params.name) return { error: "which companion? pass 'name'" };
		return { command: context.companionBin, args: ["close", params.name] };
	}

	return { error: "unknown action: " + action };
}
