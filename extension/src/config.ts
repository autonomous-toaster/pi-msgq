// Pure configuration and addressing helpers.
//
// No host imports here on purpose: everything in this module is unit-testable
// with plain Node.

/** Delivery policy: how far the subscriber trusts publisher intent. */
export type Policy = "display" | "honor";

/** Address schemes reserved by the msgq bus. */
export const RESERVED_SCHEMES = ["session", "cwd", "group", "cli"] as const;

/** A configuration or address problem that should disable the extension. */
export class ConfigError extends Error {}

/**
 * Resolve the bus path: `MSGQ_DB` when set and non-empty, otherwise the
 * default under the user's home directory.
 */
export function resolveDbPath(
	env: Record<string, string | undefined>,
	homeDir: string,
): string {
	const override = env.MSGQ_DB?.trim();
	if (override) return override;
	return `${homeDir}/.pi/agent/msgq/msgq.db`;
}

/**
 * Canonicalize an address, mirroring the CLI: a bare name becomes
 * `group/<name>`, and a prefixed address must use a reserved scheme.
 */
export function canonicalizeAddress(input: string): string {
	const trimmed = input.trim();
	if (!trimmed) throw new ConfigError("address must not be empty");

	const slash = trimmed.indexOf("/");
	if (slash === -1) return `group/${trimmed}`;

	const scheme = trimmed.slice(0, slash);
	const rest = trimmed.slice(slash + 1);
	if (!(RESERVED_SCHEMES as readonly string[]).includes(scheme)) {
		throw new ConfigError(
			`unknown address scheme '${scheme}'; expected one of session/, cwd/, group/, cli/`,
		);
	}
	if (!rest) throw new ConfigError(`address scheme '${scheme}/' requires a name`);
	return `${scheme}/${rest}`;
}

/** Parse a comma-separated address list, canonicalizing and de-duplicating. */
export function parseTopics(raw: string | undefined | null): string[] {
	if (!raw) return [];
	const out: string[] = [];
	for (const part of raw.split(",")) {
		const trimmed = part.trim();
		if (!trimmed) continue;
		const canonical = canonicalizeAddress(trimmed);
		if (!out.includes(canonical)) out.push(canonical);
	}
	return out;
}

/** Parse a policy value; the default is `display`. */
export function parsePolicy(raw: string | undefined | null): Policy {
	const value = raw?.trim();
	if (!value) return "display";
	if (value === "display" || value === "honor") return value;
	throw new ConfigError(`unknown policy '${value}'; expected 'display' or 'honor'`);
}

/**
 * Parse a comma-separated list of trusted sender addresses. Entries are compared
 * literally against a record's `from`, so write the address as it appears.
 */
export function parseTrustList(raw: string | undefined | null): string[] {
	if (!raw) return [];
	const out: string[] = [];
	for (const part of raw.split(",")) {
		const trimmed = part.trim();
		if (trimmed && !out.includes(trimmed)) out.push(trimmed);
	}
	return out;
}

/** Parse an optional address, trimmed, or undefined when absent. */
export function parseOptionalAddress(raw: string | undefined | null): string | undefined {
	const trimmed = raw?.trim();
	return trimmed ? trimmed : undefined;
}

/**
 * Parse the presence switch. Presence is enabled unless explicitly disabled.
 */
export function parsePresence(raw: string | undefined | null): boolean {
	const value = raw?.trim().toLowerCase();
	if (!value) return true;
	if (["1", "on", "true", "yes"].includes(value)) return true;
	if (["0", "off", "false", "no"].includes(value)) return false;
	throw new ConfigError(`unknown presence value '${raw}'; expected 'on' or 'off'`);
}

/** Parse an override for the presence lease lifetime, in seconds. */
export function parseTtlSeconds(raw: string | undefined | null, fallback: number): number {
	const value = raw?.trim();
	if (!value) return fallback;
	const seconds = Number(value);
	if (!Number.isFinite(seconds) || seconds <= 0) {
		throw new ConfigError(
			`invalid presence lifetime '${raw}'; expected a positive number of seconds`,
		);
	}
	return Math.floor(seconds);
}

/** The bits of session identity this extension keys on. */
export interface SessionIdentity {
	alias?: string;
	sessionId: string;
}

/** The session's own address: `session/<alias>` when named, else `session/<id>`. */
export function selfAddress(id: SessionIdentity): string {
	const self = id.alias?.trim() || id.sessionId;
	return `session/${self}`;
}

/**
 * A consumer identity that is unique per session. The alias is only a label,
 * so it is joined with the session id to keep two sessions that share an
 * alias from sharing a cursor.
 */
export function consumerId(id: SessionIdentity): string {
	const alias = id.alias?.trim();
	return alias ? `${alias}#${id.sessionId}` : id.sessionId;
}

/** The full subscription set: the session's own address plus extras, de-duplicated. */
export function subscriptionAddresses(
	id: SessionIdentity,
	extra: string[],
): string[] {
	const out = [selfAddress(id)];
	for (const address of extra) {
		if (!out.includes(address)) out.push(address);
	}
	return out;
}
