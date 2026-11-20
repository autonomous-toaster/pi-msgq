// Presence: a session's advertisement on the bus.
//
// Presence is a record, not an engine primitive: one event on the presence address, renewed for as
// long as the session runs and retracted when it stops. Any participant that can publish can
// appear, which is the point — the roster is not a pi feature. See `PROTOCOL.md`.

import { type Card, sameCard } from "./card.ts";
import { canonicalizeAddress } from "./config.ts";
import { PROTOCOL_VERSION, type MsgqRecord } from "./record.ts";
import { type PresenceState } from "./presence-text.ts";

/** The presence address when no scope is configured. */
export const PRESENCE_ADDRESS = "bus/presence";

/**
 * The presence address for a scope.
 *
 * Presence is per scope because the roster is: two teams on one bus do not appear in each other's
 * lists. The scope is part of the address, so the separation is the address grammar rather than a
 * filter a client is trusted to apply.
 */
export function presenceAddress(scope?: string): string {
	const trimmed = scope?.trim().replace(/^\/+|\/+$/g, "").toLowerCase();
	if (!trimmed) return PRESENCE_ADDRESS;
	try {
		return canonicalizeAddress(`bus/${trimmed}/presence`);
	} catch {
		return PRESENCE_ADDRESS;
	}
}

/** Default lifetime of one presence record, in seconds. */
export const PRESENCE_TTL_S = 120;

/** Default renewal cadence: one third of the lifetime. */
export const PRESENCE_RENEW_MS = 40_000;

/** What a presence record may say: a state, or `gone`, which retracts it. */
export type PresenceRecordState = PresenceState | "gone";

export interface PresenceOptions {
	/** The address this participant answers at: the record's `from`. */
	address: string;
	/** Stable identity, so two participants sharing an alias stay distinct. */
	participant: string;
	/** Display name, when there is one. */
	alias?: string;
	state: PresenceState;
	/** Lifetime of one record in seconds. Defaults to {@link PRESENCE_TTL_S}. */
	ttlS?: number;
	/** Optional flat peer-locator card. */
	card?: Card;
	/** The scope this session belongs to; presence and the roster are per scope. */
	scope?: string;
}

/**
 * A participant's presence, as a renewed record.
 *
 * Every method is a publish that may fail, and none of them throws: presence is best-effort by
 * contract, so a failure is reported through `error` and the caller decides what to say. A
 * participant that never publishes simply is not listed.
 */
/**
 * What presence needs from the thing that carries its records.
 *
 * Narrow on purpose: presence writes through the client, so it is signed like every other record,
 * and this is all it needs from a bus.
 */
export interface PresenceTransport {
	publish(address: string, record: unknown): void | Promise<void>;
}

export class Presence {
	private readonly bus: PresenceTransport;
	private readonly address_to_publish_to: string;
	private readonly participant: string;
	private readonly options: PresenceOptions;
	private state: PresenceState;
	private address: string;
	private alias: string | undefined;
	private card: Card | undefined;
	private closed = false;
	private failure: unknown;
	private published = false;

	constructor(bus: PresenceTransport, options: PresenceOptions) {
		this.bus = bus;
		this.address_to_publish_to = presenceAddress(options.scope);
		this.options = options;
		this.participant = options.participant;
		this.state = options.state;
		this.address = options.address;
		this.alias = options.alias;
		this.card = options.card;
	}

	private get ttl(): number {
		return this.options.ttlS ?? PRESENCE_TTL_S;
	}

	get currentState(): PresenceState {
		return this.state;
	}

	/** The address this participant is discoverable at. */
	get addressAt(): string {
		return this.address;
	}

	/** Whether the last publish landed. A retraction clears it. */
	get held(): boolean {
		return this.published;
	}

	/**
	 * Why the last publish failed, if it did. Presence is best-effort, so the failure is handed back
	 * here instead of thrown: the caller reports it and the session carries on.
	 */
	get error(): unknown {
		return this.failure;
	}

	/** The record this presence publishes in a given state. */
	record(state: PresenceRecordState = this.state): MsgqRecord {
		const event: Record<string, unknown> = {
			kind: "presence",
			state,
			ttl: this.ttl,
			participant: this.participant,
		};
		if (this.alias !== undefined && this.alias !== "") event.alias = this.alias;
		if (this.card && Object.keys(this.card).length > 0) event.card = this.card;
		return {
			v: PROTOCOL_VERSION,
			type: "event",
			from: this.address,
			to: this.address_to_publish_to,
			event,
		};
	}

	/**
	 * Register presence. Resolves false when the record could not be published.
	 *
	 * Awaited rather than assumed: on a server the write lands later, and a session that claimed
	 * registration before then would disagree with the roster.
	 */
	async start(): Promise<boolean> {
		if (this.closed) return false;
		return await this.publish(this.state);
	}

	/** Renew: the same record again, newer, so the lifetime restarts. */
	async renew(): Promise<boolean> {
		if (this.closed) return false;
		return await this.publish(this.state);
	}

	/** Change state, and say so at once rather than waiting for the next renewal. */
	async setState(state: PresenceState): Promise<boolean> {
		if (this.closed) return false;
		if (state === this.state) return this.published;
		this.state = state;
		return await this.publish(state);
	}

	/** Replace the locator card, and say so at once. */
	async setCard(card: Card | undefined): Promise<boolean> {
		if (this.closed) return false;
		if (sameCard(card, this.card)) return this.published;
		this.card = card;
		return await this.publish(this.state);
	}

	/** Re-register under a new address (a rename), or with a new alias. */
	async setOwner(next: { address: string; alias?: string }): Promise<boolean> {
		if (this.closed) return false;
		if (next.address === this.address && next.alias === this.alias) return this.published;
		this.address = next.address;
		this.alias = next.alias;
		return await this.publish(this.state);
	}

	/** Retract presence. Idempotent, and never fatal. */
	async stop(): Promise<void> {
		if (this.closed) return;
		this.closed = true;
		await this.publish("gone");
	}

	private async publish(state: PresenceRecordState): Promise<boolean> {
		try {
			// A file answers at once and a server answers later; awaiting both is the only way to
			// report the same thing about either.
			await this.bus.publish(this.address_to_publish_to, this.record(state));
		} catch (error) {
			// A bus write fails for reasons that have nothing to do with this presence: a full disk, a
			// handle closed underneath us. The reason is kept for the caller, and nothing is claimed.
			this.failure = error;
			this.published = false;
			return false;
		}
		this.failure = undefined;
		this.published = state !== "gone";
		return this.published;
	}
}
