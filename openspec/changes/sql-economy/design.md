# Design

## Context

See proposal.md for the measurements. Three facts constrain the fixes: the bootstrap is one
`batch_execute` of `BOOTSTRAP` + `AUTHORIZATION`; `LISTEN` already wakes both followers, so the timer
is a fallback; and `msgq.catalog` counts per address inside the function, so a caller cannot ask for
less.

## Goals / Non-Goals

**Goals:** an idle session costs one query per 30 s per address and one write per 40 s, and a command
that changes no data writes no WAL. Listing cost independent of history.

**Non-Goals:** automatic pruning (who prunes is a policy decision, not a tuning one), retention
changes, compression.

## Decisions

### 1. A schema version, not a probe

`msgq.meta` gains `schema_version`, written by the bootstrap. On connect the CLI reads the one row and
skips the DDL when the value matches the build's constant. A lower value runs the bootstrap and
updates the row; a higher value refuses with an error naming both, because a downgrade would run old
DDL over a new schema.

Alternative rejected: probing for a table (`SELECT to_regclass(...)`), which answers "some schema
exists" rather than "this schema exists" and would skip a migration that is still owed.

### 2. The timer is a watchdog, not a poll

Both followers keep `LISTEN`; the timer only covers a missed notification (a pooler in transaction
mode, a dropped connection). 30 s in the extension, 10 s in the CLI: the CLI renews its follow lock
every 30 s and a slice longer than that would starve the renewal.

Latency with a working notification is unchanged — the wake is immediate. Without one, a follower
notices within the slice, which is the honest bound to document.

### 3. The catalogue answers with or without counts

`msgq.catalog(token, counts BOOLEAN DEFAULT TRUE)`. A names or card listing passes `false` and the
function returns the address with its head only, so the cost stops depending on how many records the
address has accumulated. The default keeps every existing caller unchanged.

### 4. Presence: four times slower, same roster

`PRESENCE_TTL_S` 60 → 120, `PRESENCE_RENEW_MS` 10 s → 40 s (a third of the lifetime, as before). A
crashed session is noticed within 120 s instead of 60 s, and rows drop from 8,640 to 2,160 per day.

### 5. Checkpoint on close, locally

The local engine leaves whole pages in the write-ahead log because each CLI process opens and closes
without a checkpoint: 13.4 kB of log per 492 B of data. A checkpoint on close, best effort, folds the
log into the file and leaves the two files in proportion. Failure to checkpoint is not an error: the
next open replays the log as it always could.

## Measured after the change

Same method as the proposal: the CI image with `pg_stat_statements`, the real binary.

| figure | before | after |
|---|---|---|
| statements for a second command | 63 | **7** |
| server time for a second command | 6.7 ms | **1.1 ms** |
| WAL for a read-only command | 30 kB | **0 bytes** |
| queries for 30 s of idle `msgq follow` | 234 | **8** |
| address lookups while idle | one per tick | **none** |
| presence writes per idle day | 8,640 | **2,160** |
| local log per record | 13.4 kB | **folded on close** |
| a first command against an empty database | refused | **exit 0** |

## Risks / Trade-offs

- **A stale `schema_version` skips a migration the bus needs** → the version lives with the code that
  changes the schema, and a test asserts a bump runs the DDL once and only once.
- **A pooler that drops `LISTEN`** → the worst case becomes the watchdog slice, documented in README.
- **A slower heartbeat** → a killed session lingers in the roster for up to 120 s.
- **A checkpoint on every CLI exit** → a few milliseconds per command, against megabytes of log.
