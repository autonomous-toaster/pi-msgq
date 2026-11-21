# Design

## Context

See proposal.md — Why. The relevant current shape:

- `extension/src/consumer.ts` owns the pump: `subscribe()` starts an async loop that calls
  `readSince(cursor, 100)`, advances a local cursor, and hands each record to `onEvent`. Its
  `catch` calls `onError` and closes the watcher, so the loop cannot restart itself.
- `extension/msgq.ts` owns the consequences: `onError` maps to `disable()`, which reports to the
  UI toast, calls `teardown()`, and sets the status. `teardown()` is the same function used by
  `session_start` and `session_shutdown`.
- The persisted offset is durable and independent of the process (the affected session's cursor
  survived a hard kill at offset 367499; a fresh attach replayed all 26 pending records). What is
  missing is anything that guarantees a later attach happens.
- Presence is a separate concern with its own lease and TTL, started in `session_start` after the
  bus opens and renewed on a timer.

## Goals / Non-Goals

**Goals:**

- One throw cannot end delivery for the life of the process.
- Every session that is behind on its offset finds out, in the transcript, with a count.
- A session that cannot receive stops looking available.
- The change is confined to the extension: the bus contract, the record envelope, the delivery
  policy and the acknowledgement lifecycle are untouched.

**Non-Goals:**

- No supervisor, watchdog process, or second connection to police the first.
- No change to how a record is rendered or acknowledged.
- No new CLI surface. `msgq read` remains the manual escape hatch it was.
- No attempt to make the UI toast a reliable channel; it stays a human hint.

## Decisions

### Retry lives in the pump, not above it

The pump already owns the cursor, the watcher and the read batch; every other component would
have to reconstruct them. So `subscribe()` stops reporting a transient failure as an error and
becomes a small state machine: `attached` → `retrying` → `attached`. It reports *transitions*, not
attempts, through one callback: `onState({state, reason})`.

Rejected: re-subscribing from `msgq.ts` on every error. It would rebuild the watcher and re-read
the cursor on each hiccup, and would put the backoff in a second place.

The local cursor is kept across a retry and the persisted offset is never written on the failure
path, so a retry cannot skip or duplicate a record. A fresh attach still starts from the
persisted offset, which is what makes a restart-after-kill replay.

### Backoff is bounded, jittered, and never gives up

100 ms doubling to a 5 s cap, ±20 % jitter, reset on the first successful read. After 20
consecutive failures the pump reports once more ("still detached after N attempts") and stays at
the cap. No attempt limit: a session may outlive any outage.

### Attachment is also nudged at turn boundaries

The pump's timer is the primary mechanism; the settle hook is a nudge — if any address is not
attached when a run ends, the extension skips the remaining backoff and attaches immediately.
There is no always-on polling timer beyond what already exists.

Rejected: a periodic re-attach timer. A session that never runs and never fails does not need it,
and the watcher already covers the normal case.

### Failure and recovery are transcript records, emitted on transition only

`report()` keeps feeding the toast for the human; the model gets a `custom_message` with
`triggerTurn: false`, so a dead bus can never spawn a run. Emitted on transitions only, with a
suppression counter for repeated failures, so a flapping bus costs two lines, not two hundred.

If a run is in progress when the transition happens, the entry is queued and flushed by the
existing settle hook (`agent_before_settle` already returns `entries`); appending mid-run is
avoided for the same reason the buffered path avoids it.

The status slot reports the state machine: `msgq: attached (N)`, `msgq: detached (retrying)`,
`msgq: disabled`.

### The replay count comes from the engine, as a read

`readSince()` returns a batch, so counting from it would truncate at the batch size and the
announcement has to precede the records. The `Bus` interface gains `countSince(address, offset)`,
implemented by `HonkerBus` with a `COUNT(*)` over the stream's offsets and by `PgBus` with its own
equivalent. This is a read: it moves no cursor and changes no counter, so it stays inside
msgq-backend's "A read never moves a cursor" and "History reads are read-only".

### Presence follows attachment

Presence is started after the bus opens; it now also follows the pump's state: on `retrying` the
extension stops renewing (a retraction is a best-effort write, and the TTL of 120 s expires the
entry anyway), and on the transition back to `attached` it publishes again in the current state.
Between the two, the "presence was lost" warning is suppressed — a detachment is the reason, and
saying both would be two reports for one cause.

### Teardown is split from rebuild

`teardown()` stays the shutdown path (release subscriptions, timers, presence, engine handle,
truncate the WAL) and keeps its idempotency. Two narrower operations are introduced:
`releaseSubscription(address)` for a rebuild, and `disable(reason)` for configuration errors
only — which is now reachable from config parsing alone. A failed open is a detachment, not a
disable: the file may appear, the credential may be renewed, the server may come back.

## Risks / Trade-offs

- **Retry storm across sessions sharing one file** → jitter, a 5 s cap, and retries only after a
  failure; a healthy session adds no timer.
- **A permanently broken bus retries forever** → bounded cost (one timer per address, one report
  per 20 attempts) and the transcript says so, which is the point.
- **Transcript noise on a flapping bus** → edge-triggered emission plus a suppression counter.
- **Presence churn during a short outage** → renewals stop but the lease lives 120 s; a flap
  shorter than that never leaves the roster.
- **Two mechanisms can attach at once** (timer and settle nudge) → the subscription object
  serialises: a nudge while a retry is in flight only shortens the wait.
- **A count that races new writes** → the announced number may be smaller than what is later
  delivered; it is a hint, and the records themselves are the contract.

## Migration Plan

None needed. The persisted offset format and the consumer identity do not change, and the new
behaviour only adds attempts that the current code does not make. A session that is detached
today replays its backlog the first time the new extension attaches to it — which is also the
first thing to check after deploying.

Rollback: revert the extension; a session that has already replayed keeps its advanced cursor, so
no records are redelivered by the rollback.

## Open Questions

- Whether attachment state should also be visible to peers through the roster (an entry that says
  `retrying` rather than disappearing) — deferred; the spec only requires that a detached session
  stops claiming availability.
- Whether `countSince` deserves a CLI subcommand for parity with `read` — deferred; the extension
  is the only caller today.
