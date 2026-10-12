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
	| "buffer";

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
	if (!TURN_TYPES.includes(record.type)) return "display";
	if (!recordBody(record)) return "display";
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
}

/** Holds turn-bearing records while a run is in progress. */
export class Coalescer {
	private pending: PendingMessage[] = [];

	add(record: MsgqRecord, address: string, offset: number, commit: () => void): void {
		this.pending.push({ record, address, offset, commit });
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

/** Render one record with its provenance, and a reply hint when one was asked for. */
export function formatMessage(record: MsgqRecord, address: string): string {
	const body = recordBody(record);
	if (!wantsReply(record)) {
		return `[msgq] from ${record.from} on ${address}\n${body}`;
	}
	// A soft parenthetical at the end gets ignored: the peer answers into its own
	// transcript and the asker waits in vain. State the expected action, first, with
	// the exact tool call to make.
	return [
		`[msgq] ${record.from} asks you a question through session ${address}`,
		'Answer them using the peers tool: {"action":"reply","message":"<your answer>"}',
		"",
		body,
	].join("\n");
}

/** Render a coalesced batch with each record's provenance. */
export function formatBatch(messages: PendingMessage[]): string {
	if (messages.length === 1) {
		return formatMessage(messages[0].record, messages[0].address);
	}
	const blocks = messages.map(
		(message, index) =>
			`[${index + 1}] from ${message.record.from} on ${message.address}\n${recordBody(message.record)}`,
	);
	return [`[msgq] ${messages.length} messages`, ...blocks].join("\n\n");
}
