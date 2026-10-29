## Context

See `proposal.md` for motivation and the delta specs for the contracts.

What already exists, and what it constrains:

- The wire is produced by two implementations, not one: `cli/src/record.rs` (the author) and
  `extension/src/ack.ts` (acknowledgements, written in-process through the engine rather than through
  the CLI, for latency). The extension also writes presence, today as an engine lease.
- `cli/src/presence.rs` reads presence by parsing lease names (`msgq:participant:<kind>:<state>:<id>`)
  out of the engine's lock table. Nothing else in the CLI knows that vocabulary.
- `msgq sessions --json` is the only coupling a launcher has to the bus: `companion` blocks on it to
  learn that a spawned process is reachable, and `companion list` joins it with herdr's agent list.
- The record envelope is already open: `meta` is preserved by both readers, and an unknown `type`
  already delivers as display-only content (`extension/src/record.ts`,
  `openspec/specs/msgq-extension/spec.md` → *Record decoding*).
- Acknowledgements are already a convention over `event` records (`event.kind: "ack"`,
  `meta.ack` for the request). Only their absence from `PROTOCOL.md` is new work.
- `extension/scripts/e2e.mjs` drives headless `pi --mode rpc` through 35+ scenarios, including
  `presence`, `presence-off`, `crash-expiry`, `rename`, `squat`, `companion-list`, `skip-version` and
  `command-without-herdr`. This is the behavioural acceptance surface.
- The engine (honker) offers queues, an outbox, a scheduler and locks. Only streams, locks and update
  events are used. Locks are the piece this change removes from the model.

## Goals / Non-Goals

**Goals:**

- One place that says what the wire promises, and a way to notice when the implementations disagree.
- Presence expressible by any participant, not only by pi sessions.
- Growth bounded by an operator command rather than by hope.
- A documented boundary between what the bus promises and what the showcase demonstrates.

**Non-Goals:**

- No new envelope field, record type, address scheme or CLI verb that is removed later.
- No move of delivery policy, trust, or turn-taking into the wire.
- No packaging split, no new dependency, no daemon.
- No task/queue semantics of any kind (see D9).

## Decisions

### D1. Three duties with a precedence, and a ratchet that only turns one way

A defines the wire; B constrains it (a stranger's client must keep working); C consumes it (pi's
extension is the reference client). The operating rule: **C may never require a protocol change** —
anything pi needs lands in the extension, or in a convention composed from records that already
exist. Its test is mechanical: *can a bash script implement it with `publish` and `read`?* Yes means
convention; no means protocol, and pi's convenience is not a reason.

Alternatives: a single identity ("substrate for pi") which is what the surface has been drifting
towards; or "wire only", dropping pi's needs as a design constraint, which would strand the reference
client. The precedence is the only arrangement in which all three duties are true at once.

### D2. Three roles, so "in the repo" stops implying "promised"

The wire is the promise, the extension is the reference client, and the launcher plus the end-to-end
suite are the showcase. `companion` is the only artifact that shows two independently-owned agents on
one file with a human able to type into either, and it is a *pure* CLI consumer — it imports no
engine, constructs no record, and knows nothing about the protocol. That makes it the strongest
available evidence for B rather than a liability. It stays, and its prerequisite (herdr, tabs) stays
with it.

Alternatives: remove the launcher (loses the demonstration and the launcher-conformance role); or
promote it into the bus's identity (herdr in the requirements of a system whose selling point is one
file). Reclassifying is the only option that keeps both.

### D3. The protocol is a document plus fixtures, not generated code

`PROTOCOL.md` states the contract; `protocol/fixtures/*.json` are canonical records; both test suites
read the same fixtures. Duplication of the type list stays, and a test makes divergence fail.

Alternatives: generate the TypeScript vocabulary from the Rust one (adds a build step to a package
that today has none and is loaded as TypeScript source); a JSON Schema (describes shape, not the
tolerance *behaviour* that B actually promises); one shared crate/module (couples two languages for a
12-item list). Fixtures test the behaviour B promises — accept, reject, preserve — which is the part
that drifts.

### D4. Presence becomes records on one well-known address

Presence is published as `event` records with `kind: "presence"`, a `state`, and a declared lifetime,
to `group/presence` — a reserved group name that no session subscribes to. The roster is the newest
record per address that is younger than its declared lifetime, judged by the **stream row's
`created_at`** so no publisher's clock is trusted. Clean shutdown publishes a retraction.

Alternatives considered:

- Keep engine leases and add a CLI `presence` verb so other clients can lease. Smallest change, but it
  puts an engine concept on the command surface, keeps two vocabularies (lease names *and* records),
  and leaves the bus's own liveness dependent on a primitive only one language can call.
- Publish presence to each session's own address and have the roster scan every stream. Keeps
  presence near its owner, but makes the roster a cross-stream scan that grows with the bus.
- Records on one address (chosen): any client can appear with `publish`; retraction is explicit rather
  than a release; expiry is a comparison; and the roster is one read.

Consequences, stated rather than discovered: the extension now appends to a stream, so the old
invariant "presence never touches a stream" becomes "presence never touches a *conversation* stream";
the presence address is the bus's highest-churn stream, which is why D7 stops being optional; and a
lease's engine-computed expiry is replaced by an advertised lifetime, which is weaker (a publisher can
claim an hour) and acceptable because presence was never authentication.

### D5. Acknowledgements are documented, not changed

They are already `event.kind: "ack"` with `meta.ack` as the request. Writing that down costs a page and
buys a client that can acknowledge without pi. Adding an `ack` record type would buy nothing and spend
a permanent promise.

### D6. Correlation goes in `meta.trace`, not in the envelope

A tree of work across addresses is reconstructable from one key. `meta` is already open and preserved,
so this costs no protocol surface; an envelope field would be a permanent promise for a convenience.

### D7. Retention is an operator command, pinned by cursors

`msgq prune` refuses to remove a record at or below any consumer's cursor, preserves the newest
presence record per address, and reports before removing. Nothing prunes automatically: a session is
not an operator, and a background deleter in the extension would make data loss depend on somebody
having a tab open.

### D8. The launcher is demoted documentarily, and specified as a contract

No package split. The contract (what a launcher must start, pass, wait for, and stop) is written in
`companion/README.md`, the front door stops requiring it, and `--dry-run` is promoted as the way to
read the contract without installing its prerequisite. A package split would trade a documentation
problem for release machinery.

### D9. Queues, role pools and scheduling are dropped, with a falsifier

Work distribution, retries, deadlines and task lifecycles belong to an execution harness that can run
tools and hold a context. The engine's queue is therefore not exposed by this change. Falsifier: if a
headless pool of interchangeable workers is ever needed, that is a separate tool built on the engine's
queue, and it must not require the wire to change.

## Risks / Trade-offs

- **The roster's source changes, and a launcher plus a listing command depend on its shape** → the
  output shape is preserved and the existing e2e scenarios (`presence`, `presence-off`,
  `crash-expiry`, `rename`, `squat`, `companion-list`) are the acceptance criteria; a field added to
  `sessions --json` is allowed, a field removed is not.
- **One high-churn presence address** → one small record per session per TTL/3, retention and lag in
  `db info`, and `prune` preserving the newest record per address.
- **A publisher can advertise an hour of presence and disappear** → presence was never authority; the
  roster is a hint, and the claim that liveness is authoritative is not made anywhere.
- **Retraction lost on a crash** → the record ages out, exactly as a lease did.
- **Two sessions sharing an alias** → two entries at the same address; the roster shows one entry and
  the condition is reported rather than silently merged.
- **Pruning is the only destructive command in the system** → dry run by default, cursor-pinned,
  presence-preserving, and tested against a cursor that sits before old records.
- **The fixtures rot if they are not run** → they join the existing `DEVELOPMENT.md` test block and the
  end-to-end suite is the canary for a non-pi client.
- **The launcher contract is unenforceable in code** → the `command-without-herdr` and `companion-list`
  scenarios keep it honest, and the dry-run output is the contract's readable form.

## Migration Plan

Order: protocol and fixtures first (no behaviour change), then presence, then retention, then the
documentation. Every step is independently revertible.

Presence has a version boundary, and it is not worth engineering around: a roster reading records sees
nothing from a session still publishing leases, and vice versa. Presence is only meaningful for live
processes, and a live process survives only if it is not restarted — so the upgrade is "restart your
sessions", and the roster is empty until they come back. A dual-read window (records *and* lease names
for one release) is the fallback if that proves annoying in practice; it is not written into this
change because it adds a second vocabulary for one release and no more.

Rollback: revert the presence commit; sessions that restart publish leases again and the CLI reads
them. Records already written remain in `group/presence` and are ignored, then aged out by `prune`.
Nothing in the record contract changed, so a rollback cannot lose messages.
