// Delivery planning: maps a decoded record to a delivery decision, and coalesces
// turn-bearing records that arrive while the agent is running.

import type { Policy } from "./config.ts";
import { type MsgqRecord, recordBody } from "./record.ts";

/** What to do with a record at delivery time. */
export type DeliveryPlan =
	/** Visible in the transcript, never starts a turn. */
	| "display"
	/** Visible, and starts a turn because the agent is idle. */
	| "turn"
	/** Visible, but held until the run settles so it can be coalesced. */
	| "buffer"
	/** Delivered as input rather than as content, so the session runs it. */
	| "dispatch";

/** Record types that may act on the session under the `honor` policy. */
const TURN_TYPES = ["prompt", "steer", "follow_up"];

/**
 * Decide how to deliver a record. The policy caps publisher intent, so
 * `display` downgrades everything. A record sent by this session itself never
 * starts a turn, and a record with no body has nothing to act on.
 */
export function planDelivery(
	record: MsgqRecord,
	policy: Policy,
	selfAddress: string,
	busy: boolean,
	trusted: string[] = [],
): DeliveryPlan {
	if (policy === "display") return "display";
	if (record.from === selfAddress) return "display";
	// An empty list means no allowlist is configured; a non-empty one admits only those senders.
	if (trusted.length > 0 && !trusted.includes(record.from)) return "display";
	if (!TURN_TYPES.includes(record.type) && record.type !== "command") return "display";
	if (!recordBody(record)) return "display";
	// A command is delivered as input, and is never gathered into a batch: a batch is one message,
	// and a command has to be the whole text to be a command at all.
	if (record.type === "command") return "dispatch";
	return busy ? "buffer" : "turn";
}

/** A record waiting to be delivered as part of one coalesced message. */
export interface PendingMessage {
	record: MsgqRecord;
	/** The address the record was received on. */
	address: string;
	/** The record's stream offset. */
	offset: number;
	/** Persist progress for this record, once it is actually injected. */
	commit: () => void;
	/** The record this one refers to, when it refers to one. */
	reference?: ReferencedRecord;
}

/** Holds turn-bearing records while a run is in progress. */
export class Coalescer {
	private pending: PendingMessage[] = [];

	add(
		record: MsgqRecord,
		address: string,
		offset: number,
		commit: () => void,
		reference?: ReferencedRecord,
	): void {
		this.pending.push({ record, address, offset, commit, reference });
	}

	get size(): number {
		return this.pending.length;
	}

	get empty(): boolean {
		return this.pending.length === 0;
	}

	/** Take everything buffered, leaving the buffer empty. */
	drain(): PendingMessage[] {
		const out = this.pending;
		this.pending = [];
		return out;
	}
}

/** Whether a record asks its recipient for an answer. */
export function wantsReply(record: MsgqRecord): boolean {
	const meta = record.meta;
	return (
		typeof meta === "object" && meta !== null && (meta as { reply?: unknown }).reply === true
	);
}

/** A record another record refers to, and the id it was resolved from. */
export interface ReferencedRecord {
	/** `<address>#<offset>` of the record being referred to. */
	id: string;
	/** The record itself, when it could still be read. */
	record?: MsgqRecord;
}

/**
 * One line naming the record a message refers to.
 *
 * This goes in the content, unlike the reply affordance, because the two audiences are
 * opposite: a human reading the transcript already has the original in front of them,
 * while a session woken by a reply has no idea what it is being told about. Resolving it
 * is the whole point of the reference, so a reference that cannot be read still names the
 * id — the reader can at least tell that something is being answered.
 */
export function formatReference(reference: ReferencedRecord): string {
	const referent = reference.record;
	if (!referent) return `[about ${reference.id}]`;
	// One line, whatever shape the body had.
	const body = recordBody(referent).replace(/\s+/g, " ").trim();
	const from = `from ${referent.from}`;
	return body
		? `[about ${reference.id} ${from}: "${body}"]`
		: `[about ${reference.id} ${from}]`;
}

/**
 * The note that goes immediately before a dispatched command.
 *
 * A command must be the whole text to be a command, so the sender cannot travel inside it. This is
 * where it travels instead: one entry before, saying who is speaking — and, because provenance
 * exists so a session can tell external input from its user's own typing, saying that the next
 * message is not the user's.
 */
export function formatCommandNote(
	record: MsgqRecord,
	address: string,
	reference?: ReferencedRecord,
): string {
	const lines = [
		`[msgq] from ${record.from} on ${address}`,
		"the command that follows comes from them, not from your user",
	];
	if (reference) lines.push(formatReference(reference));
	return lines.join("\n");
}

/**
 * The entry that asks for an answer this session still owes, naming both ways out.
 *
 * Declining is an answer: a peer may look at a question and rightly conclude there is nothing to
 * say, and an entry that only accepts an answer teaches a model to pad its reports instead.
 */
export function formatOwed(owed: { from: string; id: string }): string {
	return [
		`[msgq] ${owed.from} asked you a question (${owed.id}) and you have not answered it.`,
		'Answer it with the peers tool: {"action":"reply","message":"<your answer>"}',
		"Or reply that nothing needed reporting, if that is the truth. Do not simply stop.",
	].join("\n");
}

/** Render one record with its provenance, and a reply hint when one was asked for. */
export function formatMessage(
	record: MsgqRecord,
	address: string,
	reference?: ReferencedRecord,
): string {
	const body = recordBody(record);
	const about = reference ? [formatReference(reference)] : [];
	if (!wantsReply(record)) {
		return [`[msgq] from ${record.from} on ${address}`, ...about, body].join("\n");
	}
	// A soft parenthetical at the end gets ignored: the peer answers into its own
	// transcript and the asker waits in vain. State the expected action, first, with
	// the exact tool call to make.
	return [
		`[msgq] ${record.from} asks you a question through session ${address}`,
		'Answer them using the peers tool: {"action":"reply","message":"<your answer>"}',
		...about,
		"",
		body,
	].join("\n");
}

/** Render a coalesced batch with each record's provenance. */
export function formatBatch(messages: PendingMessage[]): string {
	if (messages.length === 1) {
		return formatMessage(messages[0].record, messages[0].address, messages[0].reference);
	}
	const blocks = messages.map(
		(message, index) =>
			`[${index + 1}] from ${message.record.from} on ${message.address}${
				message.reference ? `\n${formatReference(message.reference)}` : ""
			}\n${recordBody(message.record)}`,
	);
	return [`[msgq] ${messages.length} messages`, ...blocks].join("\n\n");
}
