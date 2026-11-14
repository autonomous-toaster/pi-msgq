# Proposal

## Why

A shared bus cannot be created. Pointing msgq at a database with no schema fails before storing
anything:

    msgq topics
    msgq: cannot bootstrap the bus schema: shared engine: relation "msgq.identity" does not exist

The migration block in `sql/authorization.sql` reads `msgq.identity` and `msgq.cursor` before the
file creates `msgq.identity`. An existing database hides the ordering; an empty one rolls back the
whole bootstrap. Every shared-engine test reuses one long-lived database, so the suite has never
started from nothing, and the README's PostgreSQL walkthrough points at a fresh server.

A second defect sits beside it: on a bus with no identity registered, writes are accepted while reads
are refused, because the read functions demand a credential unconditionally.

## What Changes

- Order the schema so every table the migration block reads exists before the block runs.
- Refuse a credential only when an identity exists to check against: a bus with no identity stays
  readable, matching the write path and the wording of `db info`.
- A test that starts from an empty database, because every existing test starts from a bootstrapped one.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `msgq-backend`: a fresh database bootstraps in one connection, and the unauthenticated bus is
  readable as well as writable.

## Impact

`sql/authorization.sql` (ordering), the read functions in the same file (the credential check),
`cli/tests/pg.rs` (a case that creates its own database). No CLI, protocol or extension change.
