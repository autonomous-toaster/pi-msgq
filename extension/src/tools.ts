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
	/** Hold this turn open for the answer, instead of being woken by it. */
	wait?: boolean;
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

/** How long a waiting ask holds the turn, by default. */
export const DEFAULT_ASK_TIMEOUT_S = 60;

/**
 * The longest wait the model may ask for.
 *
 * A session has one execution slot, and a wait holds it: two sessions waiting on each other
 * cannot answer, so a wait with no limit is a deadlock with no way out. The CLI keeps
 * `--timeout 0` for scripts, which can afford to block; the model cannot, so its wait is capped
 * here where the request is made.
 */
export const MAX_ASK_TIMEOUT_S = 120;

/** The wait a request actually gets: finite, at least a second, never more than the cap. */
export function askWaitSeconds(requested?: number): number {
	if (requested === undefined || !Number.isFinite(requested)) return DEFAULT_ASK_TIMEOUT_S;
	return Math.min(MAX_ASK_TIMEOUT_S, Math.max(1, Math.floor(requested)));
}

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
		// A question is delivered by the receiving session's extension, and a record this
		// session sent itself is never allowed to start one of its turns. So a question to
		// this address could only ever wait out its window.
		if (to === context.self || to === "me") {
			return {
				error:
					"a question cannot be asked of this session itself: nothing delivers a record back to its sender, so it could only wait out its window",
			};
		}
		if (params.wait !== true) {
			// Publish and return. The answer is an ordinary inbound message, which is what
			// lets two sessions ask each other without either holding its turn.
			return {
				command: context.msgqBin,
				args: ["ask", to, params.message, "--from", context.self, "--no-wait"],
			};
		}
		return {
			command: context.msgqBin,
			args: [
				"ask",
				to,
				params.message,
				"--timeout",
				String(askWaitSeconds(params.timeoutSeconds)),
				"--from",
				context.self,
				// The record when there is one, an outcome object when there is not: a shape
				// check rather than a match on the CLI's prose.
				"--json",
			],
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

/** What a waiting ask came back with: the answer, or a reason there is none yet. */
export interface AskReport {
	/** Whether the answer was returned in this turn, and is therefore already delivered. */
	answered: boolean;
	/** What to show the model. */
	text: string;
}

/**
 * Read what `msgq ask --json` printed.
 *
 * Two shapes, never prose: the answer is the peer's raw record, and an ending without one is an
 * `outcome` object. That is what tells the caller whether the answer it is about to show has also
 * been left on the stream for the ordinary delivery path to find.
 */
export function interpretAsk(report: { ok: boolean; text: string }, to: string): AskReport {
	if (!report.ok) return { answered: false, text: report.text };
	const line = report.text
		.split("\n")
		.map((entry) => entry.trim())
		.filter(Boolean)
		.pop();
	if (line === undefined) return { answered: false, text: `no answer from ${to} yet` };

	let parsed: {
		outcome?: unknown;
		phase?: unknown;
		verdict?: { state?: unknown };
		record?: { message?: unknown };
	};
	try {
		parsed = JSON.parse(line);
	} catch {
		// Not the shape this expects: report it as it came rather than inventing one.
		return { answered: false, text: report.text };
	}
	// A record arrives wrapped with its position and its verdict, so the answer is the record inside.
	const record = parsed.record;
	if (parsed.verdict?.state === "refused") {
		return { answered: false, text: `the answer from ${to} failed its signature: ${line}` };
	}
	if (typeof record?.message === "string") {
		const unverified = parsed.verdict?.state === "unverified" ? " (unverified sender)" : "";
		return { answered: true, text: `${record.message}${unverified}` };
	}
	if (typeof parsed.outcome === "string") {
		const phase = typeof parsed.phase === "string" ? ` (${parsed.phase})` : "";
		return {
			answered: false,
			text:
				parsed.outcome === "running"
					? `${to} has not answered yet; its answer will arrive as a message`
					: `${to} finished without answering${phase}`,
		};
	}
	return { answered: false, text: report.text };
}
