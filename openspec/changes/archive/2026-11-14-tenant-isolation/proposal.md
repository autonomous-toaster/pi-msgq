# Proposal

## Why

The shared engine can host several users on one database, but only the write path checks who is
writing. Every other operation the bus offers — reading a stream, listing addresses, moving or erasing
a cursor, pruning history, writing a retention watermark — is unguarded, so one user can read,
enumerate, erase or forge another user's state by naming an address. The identity row is also keyed to
an address rather than to a principal, which conflates "who the user is" with "which sender a record
carries", and it makes a per-session credential look necessary when it is not.

## What Changes

- Identity becomes a **principal**: one row per user, whose scopes are the namespace the user may reach.
- An out-of-scope claimed sender is **refused** instead of silently rewritten, because a principal name
  is not an address to rewrite to.
- Reads, catalog listings, cursor writes and prune move behind scoped functions that take the
  credential; the runtime role loses direct access to the streams and keeps none of the table grants.
- Cursors and retention bookkeeping gain an **owner** stamped by the function, so a name collision
  cannot reach across users.
- Prune becomes a tenant operation on the tenant's own addresses, and the retention watermark is
  written only by the prune that removed the records.
- Credentials gain a **renewable lease**: renewal extends the same row, never re-mints it, and a session
  resumed after a long idle period can renew and continue.
- The writing identity is **stamped** alongside the sender, so the roster and an audit can say whose
  session a record belongs to.
- Teams are recorded as a later capability that this shape already permits: a room is an address listed
  in more than one principal's scopes, and prune authority follows ownership rather than access.

**BREAKING**: a client that relies on an out-of-scope `from` being rewritten now receives a refusal, and
a credential that has lapsed must be renewed rather than replaced.

## Capabilities

### New Capabilities

- `msgq-identity`: who a caller is, what a caller may reach, and how a claimed sender is resolved.
- `msgq-backend`: the function surface a shared deployment exposes, and what a runtime role may not do.

### Modified Capabilities

- `msgq-cli`: `consumers`, `retention`/`prune`, `db info` and cursor commands report and act within the
  caller's own namespace.
- `msgq-extension`: the extension reads through the scoped API instead of reading the streams directly.

## Impact

`sql/authorization.sql`, `cli/src/pg/` (reads, cursors, prune, catalog), `cli/src/store.rs` and
`cli/src/commands.rs` (reporting), `extension/src/pg-bus.ts` (read path), the role grants in
`sql/authorization.sql`, and the shared-engine tests. The local engine is untouched: its tenancy model is
one file per user, enforced by the operating system, and its trust statement already says the sender is
a claim.

This change depends on `pgmq-backend` landing first: it modifies behaviour that change introduces, and
one of its requirements is narrowed here.
