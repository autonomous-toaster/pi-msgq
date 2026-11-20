// Pure text and parsing for the human command surface.
//
// No host imports on purpose: the commands are thin, and everything they decide
// on their own is decided here, where it is unit-testable without a session.

import { type Card, cardSummary, parseDerivedCard } from "./card.ts";
import { canonicalizeAddress } from "./config.ts";

/** A resolved address, or why one could not be resolved. */
export type Resolved = { ok: true; address: string } | { ok: false; reason: string };

/** What a human types to mean "this session". */
export const SELF_ALIAS = "me";

/** Why an address cannot be used: the session never got one. */
export const NO_IDENTITY =
	"this session has no bus address yet: the message bus did not start";

/**
 * Resolve an address a human typed.
 *
 * `me` is this session. A bare name becomes a *session* address, not a group:
 * the thing a human addresses by a bare name is someone's session, and
 * `canonicalizeAddress` would make it a group, which is not someone to talk to.
 * An address that already carries a scheme is validated and used as given.
 */
export function resolveCommandAddress(input: string, self: string): Resolved {
	const trimmed = input.trim();
	if (!trimmed) return { ok: false, reason: "no address given" };
	if (trimmed === SELF_ALIAS) {
		return self ? { ok: true, address: self } : { ok: false, reason: NO_IDENTITY };
	}
	if (trimmed.includes("/")) {
		try {
			return { ok: true, address: canonicalizeAddress(trimmed) };
		} catch (error) {
			return { ok: false, reason: error instanceof Error ? error.message : String(error) };
		}
	}
	return { ok: true, address: `session/${trimmed}` };
}

/** What `/peer` was given: an address, a message, both, or neither. */
export interface PeerArguments {
	address?: string;
	message?: string;
}

/** One layer of matching quotes, if the text is quoted. */
function stripQuotes(text: string): string {
	if (text.length >= 2) {
		const first = text[0];
		if ((first === '"' || first === "'") && text.endsWith(first)) {
			return text.slice(1, -1).trim();
		}
	}
	return text;
}

/**
 * Split `/peer`'s arguments.
 *
 * A leading quote means the whole argument is the message and no address was
 * given, which is what `/peer "look at this"` relies on to ask which peer to
 * send to. Otherwise the first word is the address and the rest is the message.
 */
export function splitPeerArguments(raw: string): PeerArguments {
	const args = raw.trim();
	if (!args) return {};
	if (args.startsWith('"') || args.startsWith("'")) return { message: stripQuotes(args) };
	const space = args.search(/\s/);
	if (space < 0) return { address: args };
	return { address: args.slice(0, space), message: stripQuotes(args.slice(space).trim()) };
}

/** One participant, as the bus reports it. */
export interface RosterEntry {
	address: string;
	kind?: string;
	state?: string;
	alias?: string;
	expiresIn?: number;
	card?: Card;
}

/**
 * Parse `msgq sessions --json`, which is one JSON object per line. A line that is
 * not an object, or carries no address, is skipped: the roster is advisory, and a
 * malformed line must not take a command down with it.
 */
export function parseSessions(output: string): RosterEntry[] {
	const out: RosterEntry[] = [];
	for (const line of output.split("\n")) {
		const trimmed = line.trim();
		if (!trimmed) continue;
		let value: unknown;
		try {
			value = JSON.parse(trimmed);
		} catch {
			continue;
		}
		if (typeof value !== "object" || value === null) continue;
		const record = value as Record<string, unknown>;
		if (typeof record.address !== "string" || !record.address) continue;
		const entry: RosterEntry = {
			address: record.address,
			kind: typeof record.kind === "string" ? record.kind : undefined,
			state: typeof record.state === "string" ? record.state : undefined,
			alias: typeof record.alias === "string" ? record.alias : undefined,
			expiresIn: typeof record.expires_in === "number" ? record.expires_in : undefined,
		};
		if (record.card !== undefined) entry.card = parseDerivedCard(record.card);
		out.push(entry);
	}
	return out;
}

/** The tail of a roster row: its state and how long the lease has left. */
function rosterTail(entry: RosterEntry): string {
	const state = entry.state?.trim() || "-";
	const ttl = entry.expiresIn === undefined ? "" : `  ${entry.expiresIn}s`;
	const locator = cardSummary(entry.card);
	return `${state}${ttl}${locator ? `  ${locator}` : ""}`;
}

/** One peer as a line, address first so it can be read off and typed. */
export function rosterLabel(entry: RosterEntry): string {
	return `${entry.address}  ${rosterTail(entry)}`;
}

/**
 * The roster as a human reads it: the *other* participants, because `/peers me`
 * is the command for this session.
 */
export function formatRoster(entries: RosterEntry[], self: string): string {
	const others = entries.filter((entry) => entry.address !== self);
	if (others.length === 0) return "no other participant is reachable on the bus";
	const width = Math.max(...others.map((entry) => entry.address.length));
	return others
		.map((entry) => `  ${entry.address.padEnd(width)}  ${rosterTail(entry)}`)
		.join("\n");
}

/** Everything `/peers me` reports: what this session is, and what it listens to. */
export interface IdentityFacts {
	/** This session's own address, or "" when the bus never started. */
	self: string;
	/** Where the bus is. */
	bus: string;
	/** The addresses this session follows. */
	addresses: string[];
	policy: string;
	presence: "registered" | "lost" | "off";
	allowed: string[];
	card?: Card;
	/** Whether this session can write, when the engine can refuse. A read-only session is not an
	 * idle one and must not look like one. */
	write?: string;
	/** How this session encrypts, or that it does not. */
	encryption?: string;
}

/** How presence reads in the identity report. */
function presenceText(presence: IdentityFacts["presence"]): string {
	if (presence === "registered") return "registered";
	if (presence === "off") return "off (disabled)";
	return "not registered";
}

/**
 * The identity report. Every fact here is one the session already holds, so it
 * works when the bus does not — which is exactly when a human asks.
 */
export function formatIdentity(facts: IdentityFacts): string {
	const rows: Array<[string, string]> = [
		["bus", facts.bus],
		["follows", facts.addresses.length > 0 ? facts.addresses.join(", ") : "(none)"],
		["policy", facts.policy],
		["presence", presenceText(facts.presence)],
		["allow", facts.allowed.length > 0 ? facts.allowed.join(", ") : "(none set: every sender)"],
	];
	if (facts.write) rows.push(["write", facts.write]);
	if (facts.encryption) rows.push(["encryption", facts.encryption]);
	if (facts.card) {
		for (const [label, key] of [
			["project", "project"],
			["branch", "branch"],
			["worktree", "worktree"],
			["path", "path"],
		] as const) {
			const value = facts.card[key];
			if (value) rows.push([label, value]);
		}
	}
	const width = Math.max(...rows.map(([label]) => label.length));
	const header = facts.self || "(none: the message bus did not start)";
	return [header, ...rows.map(([label, value]) => `  ${label.padEnd(width)}  ${value}`)].join("\n");
}

