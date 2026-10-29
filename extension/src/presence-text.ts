// Pure presence values. No runtime imports, so it stays unit-testable.
//
// The lease-name vocabulary this module used to own is gone: presence is a record now, and the
// record's shape is described in `PROTOCOL.md` rather than encoded in a name.

/** What a participant is currently doing. */
export type PresenceState = "idle" | "busy" | "blocked";

/** Every state a record may carry. */
export const PRESENCE_STATES: PresenceState[] = ["idle", "busy", "blocked"];

export function isState(value: string): value is PresenceState {
	return (PRESENCE_STATES as string[]).includes(value);
}
