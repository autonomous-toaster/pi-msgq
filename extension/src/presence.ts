// Presence: a session's lease on the bus.
//
// Liveness comes from the engine's lease primitive, so expiry is engine-computed
// and no local clock is involved. State is encoded in the lease name, because a
// lease can carry nothing else.

import type { Bus, Lease } from "./consumer.ts";
import {
	addressFor,
	buildLeaseName,
	type PresenceKind,
	type PresenceState,
} from "./presence-text.ts";

/** Default lease lifetime. */
export const PRESENCE_TTL_S = 30;
/** Default renewal cadence, one third of the TTL. */
export const PRESENCE_RENEW_MS = 10_000;

export interface PresenceOptions {
	kind: PresenceKind;
	id: string;
	owner: string;
	state: PresenceState;
	/** Lease lifetime in seconds. Defaults to {@link PRESENCE_TTL_S}. */
	ttlS?: number;
}

/** A participant's presence, as a renewed lease. */
export class Presence {
	private readonly bus: Bus;
	private readonly options: PresenceOptions;
	private lease: Lease | undefined;
	private state: PresenceState;
	private owner: string;
	private closed = false;

	constructor(bus: Bus, options: PresenceOptions) {
		this.bus = bus;
		this.options = options;
		this.state = options.state;
		this.owner = options.owner;
	}

	private get ttl(): number {
		return this.options.ttlS ?? PRESENCE_TTL_S;
	}

	get currentState(): PresenceState {
		return this.state;
	}

	/** The lease name currently in use. */
	get name(): string {
		return buildLeaseName({
			kind: this.options.kind,
			state: this.state,
			id: this.options.id,
		});
	}

	/** The address this participant is discoverable at. */
	get address(): string {
		return addressFor(
			{ kind: this.options.kind, state: this.state, id: this.options.id },
			this.owner,
		);
	}

	get held(): boolean {
		return this.lease !== undefined;
	}

	/** Register presence. Returns false when the lease is already held elsewhere. */
	start(): boolean {
		if (this.closed) return false;
		return this.acquire();
	}

	/**
	 * Change state. The old lease is released before the new one is acquired, so
	 * a crash mid-transition leaves no row rather than a stale duplicate.
	 */
	setState(state: PresenceState): boolean {
		if (this.closed) return false;
		if (state === this.state) return this.lease !== undefined;
		this.drop();
		this.state = state;
		return this.acquire();
	}

	/** Change owner (a rename). Re-registers under the new identity. */
	setOwner(owner: string): boolean {
		if (this.closed) return false;
		if (owner === this.owner) return this.lease !== undefined;
		this.drop();
		this.owner = owner;
		return this.acquire();
	}

	/** Renew. Re-acquires once if the lease was lost. */
	renew(): boolean {
		if (this.closed) return false;
		if (!this.lease) return this.acquire();
		if (this.lease.heartbeat(this.ttl)) return true;
		this.lease = undefined;
		return this.acquire();
	}

	/** Release presence. Idempotent. */
	stop(): void {
		this.closed = true;
		this.drop();
	}

	private acquire(): boolean {
		this.lease = this.bus.acquireLease(this.name, this.owner, this.ttl);
		return this.lease !== undefined;
	}

	private drop(): void {
		this.lease?.release();
		this.lease = undefined;
	}
}
