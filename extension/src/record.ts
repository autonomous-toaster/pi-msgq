// Record envelope decoding.
//
// Consumers must tolerate types and fields they do not recognize; the payload
// is whatever the publisher wrote, so decoding is deliberately lenient about
// everything except the envelope's own identity.

export const PROTOCOL_VERSION = 1;

/** The record types this version knows. A convention, not an allow-list: an unknown type still
 * decodes, and is delivered as content. See `PROTOCOL.md`. */
export const KNOWN_TYPES = [
	"prompt",
	"steer",
	"follow_up",
	"note",
	"command",
	"event",
] as const;

/** A decoded msgq record. Unknown top-level fields are preserved. */
export interface MsgqRecord {
	v: number;
	type: string;
	from: string;
	to: string;
	message?: string;
	event?: unknown;
	reply_to?: number;
	meta?: unknown;
	[key: string]: unknown;
}

/** The outcome of decoding one stream payload. */
export type DecodeResult =
	| { ok: true; record: MsgqRecord }
	| { ok: false; reason: string };

/** Whether a record type is one this version understands. */
export function isKnownType(type: string): boolean {
	return (KNOWN_TYPES as readonly string[]).includes(type);
}

/**
 * Decode a payload into a record. Returns a skip reason instead of throwing so
 * a bad payload never stops consumption.
 */
export function decodeRecord(payload: unknown): DecodeResult {
	if (payload === null || typeof payload !== "object" || Array.isArray(payload)) {
		return { ok: false, reason: "payload is not a JSON object" };
	}

	const obj = payload as Record<string, unknown>;

	const v = obj.v;
	if (typeof v !== "number" || !Number.isInteger(v)) {
		return { ok: false, reason: "missing or invalid 'v'" };
	}
	if (v !== PROTOCOL_VERSION) {
		return { ok: false, reason: `unsupported protocol version ${v}` };
	}

	if (typeof obj.type !== "string" || !obj.type) {
		return { ok: false, reason: "missing 'type'" };
	}
	if (typeof obj.from !== "string" || !obj.from) {
		return { ok: false, reason: "missing 'from'" };
	}
	if (typeof obj.to !== "string" || !obj.to) {
		return { ok: false, reason: "missing 'to'" };
	}
	if (obj.message !== undefined && typeof obj.message !== "string") {
		return { ok: false, reason: "'message' is not a string" };
	}

	return { ok: true, record: obj as MsgqRecord };
}

/** The display body of a record: its message, or a compact event summary. */
export function recordBody(record: MsgqRecord): string {
	if (typeof record.message === "string") return record.message;
	if (record.event !== undefined) return JSON.stringify(record.event);
	return "";
}
