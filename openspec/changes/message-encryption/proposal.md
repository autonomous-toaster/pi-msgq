# Proposal

## Why

Message content is stored in the clear on both engines: in the SQLite file, and in the PostgreSQL
database behind the shared engine. A copy of that file — a backup, an external drive, a sync service —
is readable by anyone who obtains the copy, because the only protection today is the permissions of the
directory the file lives in, and permissions do not travel with a copy. The protocol already reserves
`meta.enc` for exactly this and both clients already refuse to interpret a body they cannot open, so the
socket exists and no encoding is defined.

## What Changes

- A **concrete encoding** is defined for `meta.enc`: the body is sealed with an authenticated cipher
  under a per-principal key, and the declaration names the algorithm and the key identifier.
- Key material is **passphrase-first**: the key is derived from a passphrase supplied at use time, so no
  copy of the file carries the means to open the file. An operating-system keychain and an explicit key
  file are offered as alternatives with their limits stated.
- `publish` seals the body when a key is configured; `read` opens the body; a body that cannot be opened
  is reported as an error naming the key identifier, and is never treated as content.
- The extension applies the same rule on delivery: an unopenable body is reported, starts no turn, and
  publishes no acknowledgement.
- `db info` reports the key state — configured or not, and which key identifier is in use.
- Conformance gains fixtures for an encrypted body, a tampered body and a mixed history, plus evidence
  that the stored bytes contain no plaintext body.

Not changing: the envelope, presence bodies and acknowledgement bodies stay readable, because routing,
the roster and the acknowledgement lifecycle depend on them. The store gains nothing — the body is a
string to the engine, so the local and shared engines behave identically.

## Capabilities

### New Capabilities

- `msgq-encryption`: how a body is sealed and opened, how a key is obtained, and what a reader does with a
  body the reader cannot open.

### Modified Capabilities

- `msgq-protocol`: the `meta.enc` declaration becomes concrete, and the rule for an unopenable body is
  stated as a promise rather than a reservation.
- `msgq-cli`: `publish` seals, `read` opens, `db info` reports key state.
- `msgq-extension`: delivery refuses an unopenable body and reports the key problem.

## Impact

A new module for key handling and sealing in `cli/src/` and the same in `extension/src/`, the record
layer (`cli/src/record.rs`, `extension/src/record.ts`), the publish and read commands, `db info`,
delivery in `extension/index.ts`, and the fixtures under `protocol/fixtures/`.

Metadata stays in the clear: addresses, offsets, timestamps, sizes, presence state and acknowledgement
phases. Protecting those would mean encrypting the routing the bus depends on, and the design records
that trade rather than hiding it.
