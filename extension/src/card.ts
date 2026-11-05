// Pure peer-locator helpers.
//
// The card is deliberately flat and small: project, branch, worktree, path. It
// is derived by `msgq card`, optionally overridden one field at a time through
// the peers tool, and never carries a profile or current work.

/** The only keys a reference client writes into a locator card. */
export const CARD_KEYS = ["project", "branch", "worktree", "path"] as const;

export type CardKey = (typeof CARD_KEYS)[number];

/** A derived or published locator card. An absent key is omitted. */
export type Card = Partial<Record<CardKey, string>>;

/** Per-field overrides. `null` removes an override and returns to the derived value. */
export type CardOverrides = Partial<Record<CardKey, string | null>>;

/** The reference clients keep the serialized card under this many bytes. */
export const CARD_MAX_BYTES = 512;

const encoder = new TextEncoder();

function isKey(value: string): value is CardKey {
	return (CARD_KEYS as readonly string[]).includes(value);
}

/** Keep only known, non-empty string fields from a derived card. */
export function parseDerivedCard(value: unknown): Card {
	if (typeof value !== "object" || value === null || Array.isArray(value)) return {};
	const out: Card = {};
	for (const [key, raw] of Object.entries(value)) {
		if (!isKey(key) || typeof raw !== "string") continue;
		const trimmed = raw.trim();
		if (trimmed) out[key] = trimmed;
	}
	return out;
}

/**
 * Validate a model-supplied card update. Unknown keys are refused rather than
 * silently dropped: the action is narrow by design, and a model that invents a
 * profile field should be told so.
 */
export function parseCardOverrides(
	value: unknown,
): { ok: true; overrides: CardOverrides } | { ok: false; error: string } {
	if (typeof value !== "object" || value === null || Array.isArray(value)) {
		return { ok: false, error: "card must be an object" };
	}
	const overrides: CardOverrides = {};
	for (const [key, raw] of Object.entries(value)) {
		if (!isKey(key)) {
			return {
				ok: false,
				error: `unknown card field '${key}'; expected one of ${CARD_KEYS.join(", ")}`,
			};
		}
		if (raw === null) {
			overrides[key] = null;
			continue;
		}
		if (typeof raw !== "string") {
			return { ok: false, error: `card field '${key}' must be a string or null` };
		}
		const trimmed = raw.trim();
		overrides[key] = trimmed ? trimmed : null;
	}
	return { ok: true, overrides };
}

/** Apply overrides to a derived card. `null` removes a field. */
export function mergeCard(base: Card, overrides: CardOverrides): Card {
	const out: Card = { ...base };
	for (const key of CARD_KEYS) {
		const value = overrides[key];
		if (value === null) delete out[key];
		else if (value !== undefined) out[key] = value;
	}
	return out;
}

/** Serialized size of a card, used for the soft cap. */
export function cardBytes(card: Card): number {
	return encoder.encode(JSON.stringify(card)).length;
}

/** Whether a card fits the reference-client cap. */
export function cardWithinLimit(card: Card): boolean {
	return cardBytes(card) <= CARD_MAX_BYTES;
}

/** Whether two cards are the same for publication purposes. */
export function sameCard(a: Card | undefined, b: Card | undefined): boolean {
	const left = a && Object.keys(a).length > 0 ? a : undefined;
	const right = b && Object.keys(b).length > 0 ? b : undefined;
	if (left === undefined || right === undefined) return left === right;
	return JSON.stringify(left) === JSON.stringify(right);
}

/** A short one-line locator for a roster row. */
export function cardSummary(card: Card | undefined): string {
	if (!card) return "";
	const parts = [card.project, card.branch].filter((part): part is string => Boolean(part));
	if (card.path && card.path !== ".") parts.push(card.path);
	return parts.join("  ");
}
