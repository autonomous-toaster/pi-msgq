# Proposal

## Why

The bus is one SQLite file, so its reach is the reach of the file: a second agent on a second
machine cannot be addressed at all, and the bus has no concept of a second user. The first real use
case — another agent of mine, on another machine — is therefore not served by the current backend.

Separately, retention is already de-facto minutes-to-hours in practice but is undeclared, and the
existing `prune` rule refuses to remove any record a consumer has not passed. Those two facts
together mean a single offline consumer pins history indefinitely, which is the opposite of what a
short window is for.

## What Changes

- **Two backends, one product.** The resolved bus location selects the engine: a filesystem path
  keeps the current SQLite (honker) backend, a `postgres://` DSN selects a new Postgres backend built
  on **pgmq**. Exactly one backend is active per deployment; there is no relay, no dual write, and no
  sync between the two.
- **BREAKING (spec-level): offsets are per stream.** pgmq allocates message ids per queue, so a
  record's identity becomes `(address, offset)` rather than a globally unique offset. `msgq await`
  no longer accepts a bare offset without `--on`.
- **BREAKING (spec-level): retention becomes a declared window.** Default 1 hour, configurable. A
  record older than the window is removed regardless of cursors. A consumer that falls behind the
  window loses those records, is told how many, and `msgq read` / `msgq follow` report the gap.
- **BREAKING (spec-level): `prune` stops protecting unread records.** It keeps its dry-run default
  and its newest-presence pin, and loses the "never destroy work that is still owed" rule, which a
  short window cannot satisfy.
- **Address mapping.** Each address maps to exactly one queue, reversibly and case-preservingly:
  a human-readable slug plus a stable digest suffix, plus a registry so the true address survives the
  engine's lossy, 47-character, lowercasing queue names.
- **Cursors stay cursors.** Reading never advances a position and never leases a message. The
  cursor table and non-destructive history reads are implemented on top of pgmq's tables; pgmq's
  lease API is not used on the delivery path.
- **Wake is push where possible.** `LISTEN`/`NOTIFY` via pgmq's insert trigger on the Postgres
  backend, with polling as the fallback; the existing in-process wake on SQLite.
- **Authorization on a shared backend.** A credential (pgcrypto token) is resolved by a publish
  function that stamps `from` and enforces address scopes. The SQLite backend keeps its existing
  trust statement — file permissions — and gains no credential path.
- **Roster is scoped.** Presence moves to a per-scope address so one org's roster is not another's.
- **Wire reservations for encryption.** The envelope, presence and acknowledgement bodies are
  declared always-readable; `message` and `event` content may be opaque; `meta.enc` is reserved and
  `meta.reply` is documented. No encryption is implemented here.
- **Behavioural conformance.** Fixtures gain a behavioural family that both backends must pass, so
  "same product" is tested rather than asserted.

## Capabilities

### New Capabilities

- `msgq-backend`: the storage contract both engines satisfy — backend selection and the one-bus
  rule, address-to-stream mapping, lazy creation, cursor semantics, non-destructive history reads,
  wake, prefix retention, and cross-backend conformance.
- `msgq-identity`: who may write on a backend that can authorize — credential resolution,
  bus-stamped `from`, address scopes, out-of-band transport, and loud failure.

### Modified Capabilities

- `msgq-protocol`: record identity moves from a global offset to `(address, offset)`; retention
  becomes a declared window rather than an operator act; presence is scoped; the readable/opaque
  boundary and the reserved `meta` conventions are stated.
- `msgq-cli`: backend selection from the bus location; per-stream offsets and `await` addressing;
  retention window and gap reporting replacing `prune`'s owed-work rule; `db info` reports backend,
  window and identity; publishing under authorization.
- `msgq-extension`: bus access through either backend with push-or-poll wake; cursor durability under
  a short window; distinct degradation states; scoped presence; delivery policy that refuses to act on
  an opaque body.

## Impact

- `cli/src/store.rs` becomes a trait with two implementations; `cli/src/commands.rs` gains backend,
  retention and gap handling; a new Postgres module carries the pgmq mapping, cursor table and
  authorizing publish function.
- `cli/src/config.rs` learns to distinguish a path from a DSN.
- `extension/src/consumer.ts` gains a second bus implementation; `index.ts` gains degradation states
  and the opaque-body rule; `config.ts` reads a scoped presence address.
- `PROTOCOL.md` gets three edits; `protocol/fixtures/` gains a behavioural family; `README.md` and
  `DEVELOPMENT.md` need the two-backend story.
- New dependencies: a synchronous Postgres client for the CLI, a Postgres client for the extension.
- Operational: the Postgres backend performs DDL on first publish to an address, and retention
  produces dead tuples across many tables, so autovacuum health becomes a thing to watch.

## Non-goals

- No message encryption here; only the wire reservations that make it a follow-on change rather than
  a migration.
- No claim/work-sharing delivery verb. Delivery stays broadcast.
- No relay or federation between a SQLite bus and a Postgres bus.
- No migration of existing SQLite content into Postgres.
