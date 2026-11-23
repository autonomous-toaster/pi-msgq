// Attachment reporting: what a session is told when it loses the bus, and when it gets it back.
//
// Pure: the shapes a transcript entry and the status slot take are decided here, so they can be
// tested without a TUI, and the extension has one place that says what "attached" means. A bus
// failure that only reaches a toast is a failure the model cannot see, and a model that cannot see
// it keeps talking to a peer that will never answer.

import type { AttachmentChange, AttachmentState } from "./consumer.ts";

/** The status slot, as one line. */
export const DISABLED_STATUS = "msgq: disabled";

/** One thing worth a line in the transcript. */
export interface AttachNotice {
	/** What happened, for a reader scanning the transcript. */
	kind: "detached" | "attached";
	/** The line the model reads. */
	content: string;
}

function records(count: number): string {
	return `${count} record${count === 1 ? "" : "s"}`;
}

/** "1 record was" / "3 records were": a count read aloud should not sound like a bug. */
function waitingPhrase(count: number): string {
	return `${records(count)} ${count === 1 ? "was" : "were"}`;
}

/**
 * The notice a state change earns, if any.
 *
 * Edges only: a re-attach is news after a detachment, a first attach is news only when something
 * was waiting, and a threshold notice about a failure already reported is a toast, not a second
 * entry. `previous` is the state this address was in before the change.
 */
export function attachNotice(
	change: AttachmentChange,
	address: string,
	previous: AttachmentState,
): AttachNotice | undefined {
	if (change.state === "retrying") {
		if (change.repeat) return undefined;
		const why = change.reason ? `: ${change.reason}` : "";
		return {
			kind: "detached",
			content:
				`[msgq] cannot read ${address}${why}. Retrying; records that arrive meanwhile ` +
				"are kept on the bus and delivered when it is back.",
		};
	}

	const waiting = change.waiting ?? 0;
	if (previous === "retrying") {
		return {
			kind: "attached",
			content:
				waiting > 0
					? `[msgq] reading ${address} again; ${waitingPhrase(waiting)} waiting and ${waiting === 1 ? "is" : "are"} being delivered now.`
					: `[msgq] reading ${address} again.`,
		};
	}

	if (waiting > 0) {
		return {
			kind: "attached",
			content: `[msgq] ${waitingPhrase(waiting)} waiting on ${address} and ${waiting === 1 ? "is" : "are"} being delivered now.`,
		};
	}

	// An ordinary attach is not news: silence is what a session expects at its own start.
	return undefined;
}

/**
 * The status slot for a set of subscriptions.
 *
 * A session with one address retrying is not attached, whatever the others say: the count is only
 * reported when every address is really reading, because a count is what the slot promises.
 */
export function statusFor(states: readonly AttachmentState[]): string | undefined {
	if (states.length === 0) return undefined;
	if (states.some((state) => state === "attaching" || state === "retrying")) {
		return states.some((state) => state === "attaching")
			? `msgq: attaching (${states.length})`
			: "msgq: detached (retrying)";
	}
	return `msgq: attached (${states.length})`;
}

/**
 * Whether presence may be renewed.
 *
 * A roster entry promises that a peer can reach this session. While nothing is attached that
 * promise is false, so the renewal stops and the lease expires instead of being kept alive by a
 * session that cannot hear an answer.
 */
export function presenceReachable(states: readonly AttachmentState[]): boolean {
	return states.some((state) => state === "attached");
}
