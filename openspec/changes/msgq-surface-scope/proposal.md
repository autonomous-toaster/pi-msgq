## Why

msgq is three things at once — the wire between agents, a local mailbox any process can speak, and the
substrate pi's multi-agent work runs on — and it has been growing as whichever of the three was loudest
that week. Six record types, ten CLI verbs, four address schemes, presence in engine locks, acks, a
launcher, four slash commands: the promise a stranger has to keep up with is now much larger than the
property that makes the system worth using (one SQLite file, no daemon, no network, any language).

Two symptoms, both already visible: the record vocabulary drifted one change after it was extended
(`cli/src/record.rs` knows six types, `extension/src/record.ts` knows five and `delivery.ts` matches
`"command"` as a string), and a failing presence write took a whole pi process down because a bus
promise was implemented by the client. Growth is not the problem; *what is promised to whom* is.

This change fixes the ordering — A defines, B constrains, C consumes — and rules each existing
capability into or out of the promise accordingly. It adds no protocol field and removes no feature.

## What Changes

- **Name the three duties and their precedence.** A: the wire (durable, ordered, addressed records in
  one file). B: the mailbox anyone can speak (a stable, documented, tolerant contract other clients may
  depend on). C: pi's substrate (the reference client). C may never require a protocol change; anything
  pi needs lands in the extension or in a convention built from records that already exist.
- **Name the three roles an artifact can play**, so "in the repo" stops implying "promised": the wire
  is the **promise**, the extension is the **reference client**, and the launcher plus the end-to-end
  suite are the **showcase** — replaceable, and the reason anyone believes the promise is enough.
- **Write the wire down once**: a protocol document, one canonical vocabulary both languages check
  against fixtures, and the compatibility rules that make B a promise (additive changes keep `v`,
  readers accept unknown fields, an unknown type is legal and delivers as content).
- **Move presence out of engine locks and into records.** Presence is the one bus concept a non-pi
  client cannot express at all, and its renewal is the code path that ended a process. It becomes an
  event record with a TTL, read by the same `msgq sessions` command with the same output shape.
- **State what acks already are**: an `event` record under `meta.ack`, a convention composed from the
  existing envelope. No field, no new type, no CLI verb.
- **Put correlation where it costs nothing**: `meta.trace`, documented as the key that reconstructs a
  delegation tree, rather than a new envelope field.
- **Bound what presence-as-records unblocks**: retention that respects pinned cursors, and cursor
  collection for addresses that are gone. Growth becomes a maintenance command, not a policy engine.
- **Promote the launcher instead of demoting it**: `companion` is the showcase, and its prerequisites
  (herdr, tabs) leave the bus's identity. It is documented as a **reference launcher** — the contract
  any launcher must satisfy — so a tmux, a harness, or somebody else's tool can replace it.
- **Withdraw the queue and role-pool plan.** Work distribution, retries and task lifecycles belong to an
  execution harness, not to the wire. Dropped, not deferred.

## Capabilities

### New Capabilities

- `msgq-protocol`: the contract a client other than pi may depend on — envelope, the tolerance and
  versioning rules, the conventions built from records (acks, presence, trace), and the conformance
  fixtures that keep two implementations agreeing.

### Modified Capabilities

- `msgq-cli`: the roster is derived from presence records rather than engine leases, with the same
  output shape; retention and cursor collection are added, bounded by pinned cursors.
- `msgq-extension`: presence is published as records instead of held as a lease; the record vocabulary
  is one list shared with the CLI instead of a second copy.
- `msgq-companions`: the launcher is a reference implementation of a stated contract, and the bus's
  requirements no longer include it.

## Impact

- `PROTOCOL.md` and `protocol/fixtures/` are new. `cli/src/record.rs`, `extension/src/record.ts`,
  `extension/src/delivery.ts` touch the shared vocabulary.
- `extension/src/consumer.ts` loses `Lease`, `acquireLease` and the heartbeat path; `presence.ts` and
  `presence-text.ts` move from lease names to records; `cli/src/presence.rs` and `cli/src/store.rs` read
  them.
- `cli/src/main.rs` gains retention and cursor collection; `db info` gains lag and age.
- Documentation: root `README.md`, `extension/README.md`, `companion/README.md`, `DEVELOPMENT.md`.
- No protocol field is added or removed, no record type is added or removed, no CLI verb is removed,
  and no wire-visible behaviour changes for a client that only publishes and reads.
- The end-to-end suite (`extension/scripts/e2e.mjs`) is the acceptance surface for the presence change:
  `presence`, `presence-off`, `crash-expiry`, `rename`, `squat`, `companion-list`, `skip-version` and
  `command-without-herdr` must stay green.

Explicitly not in this change:

- No queue, no role pool, no claim/ack task machinery, no scheduler in the CLI.
- No new record type, no new envelope field, no `v` bump, no new address scheme.
- No change to delivery policies (`display`, `honor`), to the trust list, or to how a turn is started.
- No removal of `ask`, `await`, `command`, or the four slash commands.
- No packaging split of the extension, and no change to the launcher's behaviour.
- No movement of work into an execution harness: this change only stops the wire from pretending to be
  one.
