## Why

The bus is durable but blind. A message published to `session/x` is delivered whenever `x`
connects, so delivery is already correct — but nothing can tell whether `x` is online *now*.
`msgq consumers` lists cursors with no liveness at all, so a sender discovers addresses by
guessing, and an operator has no roster. Discovery is the missing piece, and it is the first
step toward the multi-agent ecosystem this bus is meant to carry.

## What Changes

- The extension registers a **time-limited lease** for each live session, using the engine's
  existing lock primitive, and renews it while the session runs.
- The lease **name** encodes the participant kind and its current state; the lease **owner** is
  the identity the extension already uses for cursors.
- State is `idle`, `busy`, or `blocked`, updated on transition and on a mid-session rename.
- The extension gains a **scoped write exception**: it may write leases, and still cannot
  publish messages.
- The CLI gains `msgq sessions`, which lists live participants from the lease table.
- Presence is **advisory**: it never gates delivery, and an offline session still receives
  everything durably.

Explicitly not in this change:

- No `msgq sessions --watch` (one-shot listing only).
- No symmetric registration: the CLI does not register itself as a participant yet, though the
  lease name is shaped so it can later.
- No metadata the lease cannot carry (cwd, pid, model), and no `left` versus `lost` distinction.
- No resource coordination (file reservations, task claims); leases are kept general for that
  later work but no such lease is introduced here.
- No change to the message record contract, no new record type, no schema change, and no
  protocol version bump.
- No security hardening (sender allowlist, loop guard); that is a separate change.

## Capabilities

### New Capabilities

<!-- None. Presence extends existing capabilities with new requirements. -->

### Modified Capabilities

- `msgq-extension`: add presence registration and renewal, state transitions, rename handling,
  the scoped write exception, and presence-specific resilience.
- `msgq-cli`: add the `msgq sessions` listing of live participants.

## Impact

- `extension/index.ts` and `extension/src/`: lease lifecycle, state transitions, rename handling.
- `cli/src/`: a new `sessions` subcommand, a `Store` query over the engine lease table, and a
  name/owner parser.
- Uses the engine's lock primitive that both bindings already expose (`tryLock` / `heartbeat` /
  `release`); no new dependency.
- No bus schema change and no migration. Leases expire on their own, so no pruning is required.
- Both component READMEs gain a presence section.
