// Pure presence naming and parsing. No runtime imports, so it stays unit-testable.

/** Who is being registered. */
export type PresenceKind = "session" | "cli";

/** What a participant is currently doing. */
export type PresenceState = "idle" | "busy" | "blocked";

/** Lease name prefix for every participant lease. */
export const PRESENCE_PREFIX = "msgq:participant:";

/** The parts encoded in a lease name. */
export interface LeaseParts {
	kind: PresenceKind;
	state: PresenceState;
	id: string;
}

export function isKind(value: string): value is PresenceKind {
	return value === "session" || value === "cli";
}

export function isState(value: string): value is PresenceState {
	return value === "idle" || value === "busy" || value === "blocked";
}

/** Build a lease name: `msgq:participant:<kind>:<state>:<id>`. */
export function buildLeaseName(parts: LeaseParts): string {
	return `${PRESENCE_PREFIX}${parts.kind}:${parts.state}:${parts.id}`;
}

/**
 * Parse a lease name. `kind` and `state` are positional; `id` is the trailing
 * remainder, so an id containing `:` still parses.
 */
export function parseLeaseName(name: string): LeaseParts | undefined {
	if (!name.startsWith(PRESENCE_PREFIX)) return undefined;
	const rest = name.slice(PRESENCE_PREFIX.length);
	const first = rest.indexOf(":");
	if (first < 0) return undefined;
	const second = rest.indexOf(":", first + 1);
	if (second < 0) return undefined;

	const kind = rest.slice(0, first);
	const state = rest.slice(first + 1, second);
	const id = rest.slice(second + 1);
	if (!isKind(kind) || !isState(state) || id.length === 0) return undefined;
	return { kind, state, id };
}

/** The alias carried by an owner string (`<alias>#<id>`), if any. */
export function aliasFromOwner(owner: string): string | undefined {
	const index = owner.indexOf("#");
	if (index <= 0) return undefined;
	const alias = owner.slice(0, index).trim();
	return alias.length > 0 ? alias : undefined;
}

/** The address a participant is discoverable at: `<kind>/<alias-or-id>`. */
export function addressFor(parts: LeaseParts, owner: string): string {
	const self = aliasFromOwner(owner) ?? parts.id;
	return `${parts.kind}/${self}`;
}
