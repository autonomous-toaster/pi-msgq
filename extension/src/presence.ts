// Presence: a session's advertisement on the bus.
//
// Presence is a record, not an engine primitive: one event on the presence address, renewed for as
// long as the session runs and retracted when it stops. Any participant that can publish can
// appear, which is the point — the roster is not a pi feature. See `PROTOCOL.md`.

import type { Bus } from "./consumer.ts";
import { PROTOCOL_VERSION, type MsgqRecord } from "./record.ts";
import { type PresenceState } from "./presence-text.ts";

/** The address every participant publishes presence to, and the roster reads. */
export const PRESENCE_ADDRESS = "group/presence";

/** Default lifetime of one presence record, in seconds. */
export const PRESENCE_TTL_S = 30;

/** Default renewal cadence: one third of the lifetime. */
export const PRESENCE_RENEW_MS = 10_000;

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
}

/**
 * A participant's presence, as a renewed record.
 *
 * Every method is a publish that may fail, and none of them throws: presence is best-effort by
 * contract, so a failure is reported through `error` and the caller decides what to say. A
 * participant that never publishes simply is not listed.
 */
export class Presence {
	private readonly bus: Bus;
	private readonly participant: string;
	private readonly options: PresenceOptions;
	private state: PresenceState;
	private address: string;
	private alias: string | undefined;
	private closed = false;
	private failure: unknown;
	private published = false;

	constructor(bus: Bus, options: PresenceOptions) {
		this.bus = bus;
		this.options = options;
		this.participant = options.participant;
		this.state = options.state;
		this.address = options.address;
		this.alias = options.alias;
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
		return {
			v: PROTOCOL_VERSION,
			type: "event",
			from: this.address,
			to: PRESENCE_ADDRESS,
			event,
		};
	}

	/** Register presence. Returns false when the record could not be published. */
	start(): boolean {
		if (this.closed) return false;
		return this.publish(this.state);
	}

	/** Renew: the same record again, newer, so the lifetime restarts. */
	renew(): boolean {
		if (this.closed) return false;
		return this.publish(this.state);
	}

	/** Change state, and say so at once rather than waiting for the next renewal. */
	setState(state: PresenceState): boolean {
		if (this.closed) return false;
		if (state === this.state) return this.published;
		this.state = state;
		return this.publish(state);
	}

	/** Re-register under a new address (a rename), or with a new alias. */
	setOwner(next: { address: string; alias?: string }): boolean {
		if (this.closed) return false;
		if (next.address === this.address && next.alias === this.alias) return this.published;
		this.address = next.address;
		this.alias = next.alias;
		return this.publish(this.state);
	}

	/** Retract presence. Idempotent, and never fatal. */
	stop(): void {
		if (this.closed) return;
		this.closed = true;
		this.publish("gone");
	}

	private publish(state: PresenceRecordState): boolean {
		try {
			this.bus.publish(PRESENCE_ADDRESS, this.record(state));
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
