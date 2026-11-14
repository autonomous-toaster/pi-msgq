# Proposal

## Why

Measured against the CI image with `pg_stat_statements`, on the real write path:

    one `msgq topics`           63 statements, 57 of them schema installation
    one `msgq publish`          81 statements
    WAL per command              30 kB, for a read that changes nothing
    pg_proc per command          16 kB, 18 functions rewritten
    idle `msgq follow`          7.8 queries/second
    idle subscription            1 query/second per address
    presence heartbeat           1 write/10 s, 492 B/row, never pruned
    `topics` on 26k rows        +2.4-3.7 ms per address (COUNT(*) per address)

An agent left running for a week therefore costs ~30 MB of rows and ~43 MB of WAL for its own
presence, plus ~64 MB of WAL and catalog churn from commands that write no data.

## What Changes

- A schema version in `msgq.meta` skips the bootstrap when the installed schema is current, removing
  ~60 statements and 30 kB of WAL from every invocation.
- The wait slice becomes a watchdog: `LISTEN` wakes a follower, and the timer only verifies. Idle
  reads drop from 7.8/s to 0.1/s in the CLI and from 1/s to 1/30 s per address in the extension.
- The CLI follow loop caches the queue name instead of resolving the address on every tick.
- The presence heartbeat slows to one write per 40 s against a 120 s lifetime: four times fewer rows
  for the same roster.
- A names or card listing stops counting: the count is what makes a listing cost grow with history.
- The local engine checkpoints the write-ahead log on close, so the file holds ~376 B/row instead of
  13.4 kB/row.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `msgq-backend`: the bootstrap runs once per schema version, and the catalogue can answer without
  counting.
- `msgq-cli`: a listing without counts, and a follower that waits on a notification.
- `msgq-extension`: the subscription waits on a notification, and the heartbeat is slower.

## Impact

`cli/src/pg/schema.rs` (version guard), `sql/authorization.sql` (catalogue without counts),
`cli/src/commands.rs` (wait slice, queue cache, names path), `cli/src/pg/mod.rs` (names path),
`extension/src/pg-bus.ts` (idle wait), `extension/src/presence.ts` (cadence),
`cli/src/store.rs` (checkpoint on close). Verification is the measurement above, repeated.
