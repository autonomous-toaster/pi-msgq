// Record envelope decoding.
//
// Consumers must tolerate types and fields they do not recognize; the payload
// is whatever the publisher wrote, so decoding is deliberately lenient about
// everything except the envelope's own identity.

import { additionalData, ALG, openBytes, sealBytes, type SealingKey } from "./seal.ts";

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

/**
 * The key identifier of a body this build cannot interpret, when the record declares one.
 *
 * A record whose body is encoded declares it in `meta.enc`. Presence and acknowledgement bodies are
 * never opaque: the roster and the lifecycle read them, and `PROTOCOL.md` calls both public
 * metadata. A declaration on those bodies is therefore ignored rather than obeyed.
 */
export function opaqueBody(record: MsgqRecord): string | undefined {
	const event = record.event as { kind?: unknown } | undefined;
	const kind = event?.kind;
	if (record.type === "event" && (kind === "presence" || kind === "ack")) return undefined;

	const meta = record.meta as { enc?: unknown } | undefined;
	const enc = meta?.enc as { alg?: unknown; kid?: unknown } | undefined;
	if (typeof enc?.alg !== "string") return undefined;
	// An algorithm without a key id is still opaque: nothing here can read it.
	return typeof enc.kid === "string" ? enc.kid : "unknown";
}

/** The declaration of an encoded body in a record, if the record carries one. */
export function encInfo(record: MsgqRecord): { alg: string; kid: string } | undefined {
	const meta = record.meta as { enc?: unknown } | undefined;
	const enc = meta?.enc as { alg?: unknown; kid?: unknown } | undefined;
	if (typeof enc?.alg !== "string") return undefined;
	return { alg: enc.alg, kid: typeof enc.kid === "string" ? enc.kid : "unknown" };
}

/** Which field holds a body that may be sealed. Presence and acknowledgement bodies never are. */
function sealableField(record: MsgqRecord): "message" | "event" | undefined {
	if (opaqueBody(record) === undefined && encInfo(record) === undefined) {
		const event = record.event as { kind?: unknown } | undefined;
		if (record.type === "event" && (event?.kind === "presence" || event?.kind === "ack")) {
			return undefined;
		}
	}
	if (typeof record.message === "string") return "message";
	if (record.event !== undefined && typeof record.event === "object" && record.event !== null) {
		return "event";
	}
	return undefined;
}

/** The outcome of opening one record. A failure carries the reason a reader should report. */
export type OpenResult =
	| { ok: true; record: MsgqRecord }
	| { ok: false; kid: string; reason: string };

/**
 * Open a sealed body, or say why it cannot be opened.
 *
 * The declaration is dropped from the returned record: a body that is now readable must not still
 * claim to be encoded.
 */
export function openRecord(record: MsgqRecord, key?: SealingKey): OpenResult {
	const info = encInfo(record);
	if (!info) return { ok: true, record };
	if (info.alg !== ALG) {
		return { ok: false, kid: info.kid, reason: `it is encoded with ${info.alg}, which this build does not implement` };
	}
	if (!key) {
		return { ok: false, kid: info.kid, reason: "no passphrase is configured for this session" };
	}
	const enc = (record.meta as { enc: { nonce?: string; field?: string } }).enc;
	const field = enc.field === "event" ? "event" : "message";
	const body = field === "message" ? record.message : JSON.stringify(record.event);
	if (typeof body !== "string") {
		return { ok: false, kid: info.kid, reason: `the record carries no ${field} body` };
	}
	try {
		const plaintext = openBytes(enc.nonce ?? "", body, additionalData(record), key).toString("utf8");
		const opened: MsgqRecord = { ...record, [field]: field === "event" ? JSON.parse(plaintext) : plaintext };
		const meta = { ...(record.meta as Record<string, unknown>) };
		delete meta.enc;
		opened.meta = meta;
		return { ok: true, record: opened };
	} catch (error) {
		return { ok: false, kid: info.kid, reason: `${String((error as Error).message ?? error)} (kid=${info.kid})` };
	}
}

/** Seal the body of a record. Returns the record unchanged when it has no sealable body. */
export function sealRecord(record: MsgqRecord, key: SealingKey): MsgqRecord {
	const field = sealableField(record);
	if (!field) return record;
	const plaintext = field === "message" ? (record.message ?? "") : JSON.stringify(record.event);
	const { nonce, body } = sealBytes(Buffer.from(plaintext, "utf8"), additionalData(record), key);
	const meta = { ...((record.meta as Record<string, unknown>) ?? {}) };
	meta.enc = { alg: ALG, kid: key.kid, nonce, field };
	return { ...record, [field]: body, meta };
}
