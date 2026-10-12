// Acknowledgement records and request parsing.
//
// Pure: no runtime imports, so it stays unit-testable.

/** The lifecycle points an acknowledgement can report. */
export type AckPhase = "received" | "started" | "completed";

/** How a record was delivered, reported on `received`. */
export type AckDelivery = "display" | "turn" | "queued";

/** How a run ended, reported on `completed`. */
export type AckOutcome = "ok" | "error" | "aborted";

/** Every phase, in order. */
export const ACK_PHASES: AckPhase[] = ["received", "started", "completed"];

/** Discriminator carried in the acknowledgement's `event` object. */
export const ACK_KIND = "ack";

export function isAckPhase(value: string): value is AckPhase {
	return (ACK_PHASES as string[]).includes(value);
}

/** Whether a decoded record is itself an acknowledgement. */
export function isAckRecord(record: { type?: unknown; event?: unknown }): boolean {
	if (record?.type !== "event") return false;
	const event = record.event;
	return (
		typeof event === "object" &&
		event !== null &&
		(event as { kind?: unknown }).kind === ACK_KIND
	);
}

/**
 * Read the acknowledgement request from a record's `meta`.
 *
 * `true` requests every phase; a string names a comma-separated subset. Anything
 * unrecognised yields no request, so a malformed hint never produces traffic.
 */
export function parseAckRequest(meta: unknown): AckPhase[] | undefined {
	if (typeof meta !== "object" || meta === null) return undefined;
	const raw = (meta as { ack?: unknown }).ack;
	if (raw === true) return [...ACK_PHASES];
	if (typeof raw !== "string") return undefined;

	const phases: AckPhase[] = [];
	for (const part of raw.split(",")) {
		const name = part.trim().toLowerCase();
		if (isAckPhase(name) && !phases.includes(name)) phases.push(name);
	}
	return phases.length > 0 ? phases : undefined;
}

export interface AckOptions {
	/** The acknowledging session's address. */
	from: string;
	/** The address that asked for the acknowledgement. */
	to: string;
	/** The original record's stream offset. */
	replyTo: number;
	phase: AckPhase;
	/** Set on `received`. */
	delivery?: AckDelivery;
	/** Set on `completed`. */
	outcome?: AckOutcome;
}

/** Build an acknowledgement record, ready to publish. */
export function buildAck(options: AckOptions): Record<string, unknown> {
	const event: Record<string, unknown> = { kind: ACK_KIND, phase: options.phase };
	if (options.delivery !== undefined) event.delivery = options.delivery;
	if (options.outcome !== undefined) event.outcome = options.outcome;

	return {
		v: 1,
		type: "event",
		from: options.from,
		to: options.to,
		reply_to: options.replyTo,
		event,
	};
}

/**
 * Run an acknowledgement publish. A failure is reported rather than propagated: an
 * acknowledgement must never be able to break consumption.
 */
export function publishSafely(publish: () => void, onError: (error: unknown) => void): void {
	try {
		publish();
	} catch (error) {
		onError(error);
	}
}

/** The identity of an acknowledgement: original offset, sender, and phase. */
export function ackId(replyTo: number, phase: AckPhase): string {
	return `${replyTo}:${phase}`;
}

/** Whether a phase ends the lifecycle for a record. */
export function isTerminalAck(phase: AckPhase, delivery?: AckDelivery): boolean {
	return phase === "completed" || (phase === "received" && delivery === "display");
}
