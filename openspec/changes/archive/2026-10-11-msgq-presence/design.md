## Context

See `proposal.md` for motivation and the two delta specs for the behavior contract.

Three facts from the engine and the existing code shape the approach:

- The engine already ships a **lease** primitive: `_honker_locks(name PRIMARY KEY, owner,
  expires_at)`, exposed in both bindings as `tryLock` / `heartbeat` / `release`. Acquiring deletes
  an expired row first, so expiry needs no sweep; a same-owner re-acquire does **not** refresh the
  TTL, so renewal must go through heartbeat.
- `_honker_results` is a TTL'd key/value store whose `get` honors expiry, but its integer key has
  no namespace and the table is semantically about queue job results.
- The extension already holds an identity: the consumer id (`<alias>#<session-id>`, or the session
  id alone), and it already has a stable session-scoped address (`session/<alias-or-id>`).

## Goals / Non-Goals

**Goals:**

- Discovery: a sender or operator can see which sessions are online, and in what state.
- Correct-by-default liveness: crash-safe without a sweep, no daemon, no local clock arithmetic.
- Sparse: storage proportional to live participants, not to events.
- No new schema, no migration, no protocol change.

**Non-Goals:**

- A live watch mode (`--watch`).
- Symmetric registration of CLI participants.
- Metadata the lease cannot carry (cwd, pid, model), and a `left` versus `lost` distinction.
- Resource coordination (file reservations, task claims) — leases are kept general for it, but no
  such lease is introduced here.
- Security hardening, and any herdr integration.

## Decisions

### D1. Presence is a lease, not a custom table

Use the engine's lock primitive rather than a `msgq_presence` table.

- Alternatives: a `msgq`-owned presence table; `_honker_results` as a TTL'd JSON blob; a
  heartbeat topic backed by a durable stream.
- Rationale: a lease *is* liveness — TTL, renewal, and explicit release — with the expiry computed
  by the engine, so no component does clock arithmetic and no sweep is needed. It is sparse (one
  row per participant). A heartbeat topic would put state in an append-only log, making it grow.
- The user's direction: "leverage honk features first instead of a custom table."

### D2. State is encoded in the lease name

`msgq:participant:<kind>:<state>:<id>`, with `kind ∈ {session, cli}` and
`state ∈ {idle, busy, blocked}`.

- Alternatives: two leases (a stable liveness lease plus a mutable state lease); state in a
  TTL'd result.
- Rationale: a lease carries only name and owner, so state has to live in one of them. The name is
  prefix-enumerable, which makes the roster a single query, and `session` ids are UUIDs, so the
  trailing `id` segment is unambiguous. Parsing rule: split on `:`, take `kind` and `state` by
  position, and treat the remainder as `id`.
- `kind` costs one segment and makes symmetric presence a new row later rather than a redesign.

### D3. Release, then acquire, on a state change

- Alternative: acquire the new state before releasing the old.
- Rationale: the two are equally brief, but they fail differently. A crash during
  acquire-then-release leaves **two** rows for up to the TTL (a lingering false positive); a crash
  during release-then-acquire leaves **none** (self-healing, and correct). The cost of
  release-first is a sub-millisecond window in which the session looks absent, which a one-shot
  listing will not observe.

### D4. TTL 30s, renew 10s

- Rationale: three beats of slack means one dropped renewal is never a false death, and a real
  death is detected within about 30 seconds. The write cost is one small UPDATE per session per
  10 seconds.

### D5. Owner is the existing consumer identity; the address is derived from it

- Rationale: one identity concept across cursors and leases. The roster derives
  `session/<alias-or-id>` from the owner, so the address it prints is literally the address a
  sender publishes to.

### D6. Scoped write exception

The extension may write leases and nothing else.

- Rationale: presence requires a writer, and the read-only stance was a scope decision rather than
  a constraint. Keeping the exception to leases preserves the property that matters — the
  extension cannot inject messages or move cursors.

### D7. Presence is advisory and never gates delivery

- Rationale: durable streams already deliver to offline sessions. Liveness is a discovery layer,
  so a stale or forged entry can only mislead a human, never change whether a message arrives.
  This is also why presence is documented as unverified, like `from`.

### D8. The CLI reads the lease table through the raw-query escape hatch

- Rationale: neither binding exposes lease enumeration, and the same escape hatch is already used
  for `_honker_stream` in `msgq topics`. The coupling is a documented, contained engine-table
  read, isolated in the store module.

### D9. No protocol or record change

- Rationale: presence lives entirely in engine locks. No new record type, no `protocol_version`
  bump, and no migration, so older readers are unaffected.

### D10. Lifecycle and timer hygiene

Acquire in `session_start` for every reason; release inside the existing idempotent `teardown()`;
start the renewal interval in `session_start` (never in the factory), clear it in `teardown()`, and
`unref()` it so it cannot hold a print-mode process open.

- Rationale: reload replaces the runtime, so acquisition and release must be idempotent and
  centralized. The extension contract forbids starting timers in the factory.

### D11. Presence is on by default, with an opt-out

- Rationale: discovery is the point of the feature. A user who does not want sessions writing to
  the bus gets a configuration switch, and disabling presence never disables consumption.

## Risks / Trade-offs

| Risk | Mitigation |
|---|---|
| A process squats a session's lease name | Report and keep consuming; presence is advisory, so a squatter can mislead but not disable the session (D7) |
| A crash between release and acquire | Leaves no row; the session reappears on its next start |
| Two lease rows for one participant (crash path) | The listing groups by id and shows one entry, preferring `busy` over `idle` over `blocked` |
| Name parsing breaks if an id contains `:` | Document that `kind` and `state` are positional and `id` is the trailing remainder; session ids are UUIDs today |
| A rename races the renewal beat | Rename triggers the same re-acquire path as a state change, and renewal reconciles from the current name |
| Renewal silently stops (lost lease) | Heartbeat reports loss, which drives a re-acquire attempt and a UI report |
| Reading an engine table directly couples us to its layout | Same accepted trade-off as `msgq topics`; confined to the store module |

## Migration Plan

None required.

- Leases expire on their own, so no cleanup ships with this change.
- Rollback is disabling presence, or reverting the extension; stale leases disappear within the TTL.
- Older CLI versions simply do not know the `sessions` command; nothing else changes for them.

## Open Questions

- `msgq sessions --watch` (transition notifications) — deferred; a live watch would make the
  release/acquire window observable and may want a debounce.
- Symmetric registration of CLI participants — the name is shaped for it (`kind`), the behavior is
  not implemented.
- Richer metadata (cwd, model, pid) — would need a TTL'd result alongside the lease; deferred.
- herdr integration — deliberately out of scope; the state vocabulary matches so a bridge is a
  mapping later, but reconciling two sources of "what is this agent doing" needs its own analysis.
