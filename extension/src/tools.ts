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

/** The narrower list `list` falls back to when the CLI predates the locator column. */
export const PLAIN_LIST: string[] = ["sessions"];

/**
 * Whether a failed report calls for the fallback: only a command line the CLI refused as
 * malformed does. A bus that cannot be read is the bus's problem, and a second attempt
 * would only repeat it.
 */
export function shouldRetryPlainList(
	command: Command,
	report: { ok: boolean; text: string },
): boolean {
	return !report.ok && command.args.includes("--wide") && /unexpected argument|--wide/i.test(report.text);
}

/** Arguments accepted by the peers tool. */
export interface PeerParams {
	action: "list" | "send" | "ask" | "reply";
	to?: string;
	message?: string;
	timeoutSeconds?: number;
	/** Send a command for the peer to run, instead of a message for it to read. */
	command?: boolean;
}

/** What the peers planner needs to know about this session. */
export interface PeerContext {
	/** The msgq binary to invoke. */
	msgqBin: string;
	/** This session's own address. */
	self: string;
	/** The most recently received message, for replies. */
	lastInbound?: { from: string; offset: number };
	/** Whether an outbound message should ask to be told when the peer's run completes. */
	expectCompletion?: boolean;
}

/** Default window for ask, in seconds. */
export const DEFAULT_ASK_TIMEOUT_S = 600;

/**
 * Refuse to build a command without an identity. Sending as nobody produces a record
 * with an empty sender, which no peer can answer.
 */
const NO_IDENTITY = "this session has no bus address yet: the message bus did not start";

/** Plan a peers action. */
export function planPeerAction(params: PeerParams, context: PeerContext): Command | PlanError {
	const action = params.action as string;
	if (action === "list") {
		// The model reads the same row a human does, locator column included: without it a
		// peer is a session id and nothing else. Falling back to PLAIN_LIST covers a CLI
		// that predates the column.
		return { command: context.msgqBin, args: ["sessions", "--wide"] }; // needs no identity
	}

	// A command must be the whole message, so nothing can be wrapped around it: not a reply
	// instruction, and not a question. Only sending one makes sense.
	if (params.command && action !== "send") {
		return {
			error:
			action === "ask"
				? "a command cannot be asked: a command has to be the whole message, so put the request for an answer in its own arguments"
				: "only 'send' takes a command",
		};
	}

	if (!context.self) return { error: NO_IDENTITY };

	if (action === "send") {
		if (!params.to) return { error: "no recipient: pass 'to'" };
		if (!params.message) return { error: "no message to send" };
		const to = params.to;
		// Send as this session, not as the CLI's default sender: the peer's trusted
		// sender list, and its notion of who to answer, both depend on it.
		const args = [
			"publish",
			to,
			params.message,
			"--type",
			params.command ? "command" : "prompt",
			"--from",
			context.self,
		];
		// A sender that chose not to wait still learns the peer's run finished, including when it
		// finished without answering. Not for a message to oneself: there is nothing to wait for,
		// and the acknowledgement would come straight back.
		if (context.expectCompletion && to !== context.self) args.push("--ack", "completed");
		return { command: context.msgqBin, args };
	}

	if (action === "ask") {
		if (!params.to) return { error: "no recipient: pass 'to'" };
		if (!params.message) return { error: "no question to ask" };
		const to = params.to;
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
