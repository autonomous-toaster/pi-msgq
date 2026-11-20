# Proposal

## Why

A record's sender is asserted by the bus, not proven to the reader. On a local sqlite bus that is
fine: one uid, one trust domain, and file permissions are the boundary. On a shared Postgres bus it
means every participant trusts whoever operates the database to stamp the sender honestly, and
within one principal every session presents the same bearer token, so nothing distinguishes two
agents of the same principal from each other.

The threat we are answering is co-tenancy: another user's agents on the same bus, plus a database
operator who is not us. The threat we are not answering, for now, is a hostile process running as
the same uid on this laptop.

The mechanism is not the bus. It is the reader. A record carries a signature that the receiving
client checks against a key it learned out of band, so the bus can drop and reorder records but
cannot impersonate a sender, and cannot rewrite history without being caught.

## What Changes

- A record MAY carry a public key and a signature over its canonical sender, its address and its
  body. Signatures are ed25519, keys are per principal, one key per principal.
- The reading client verifies and reports one of three outcomes per record: `trusted`,
  `unverified`, or `refused` with a reason.
- Trust anchors live in a keyring scoped to the bus, written only by a human-initiated act.
  Nothing is ever trusted automatically, and nothing about trust travels over the bus.
- Three trust layers — environment, pi session, durable keyring — resolved with a fixed precedence
  and a reported conflict when two layers disagree.
- **BREAKING** for unsigned history: a bus is strict by default and refuses an unsigned record.
  Reading legacy or mixed history requires `MSGQ_INSECURE=1` for that invocation.
- Rotation retires a key: records signed by a retired key are refused, and rotation reports how many
  stored records it makes unreadable.
- The pi extension gains `/msgq-trust`, a command rather than a tool, so that trusting a peer is
  always an act of the person at the keyboard.
- The claim in `msgq-identity` that a session can never speak as a sibling session is corrected:
  that is true of the bus's scope check and false of a shared bearer token.

## Capabilities

### New Capabilities

- `msgq-trust`: how a reader decides whether to believe a sender — key material and where it lives,
  the trust layers and their precedence, the three verification outcomes, rotation and revocation,
  and the bus-scoped keyring. This is the reader's opinion, deliberately separate from the bus's
  authority.

### Modified Capabilities

- `msgq-protocol`: the record gains an optional signature and key, the signed bytes are fixed and
  canonicalized, the envelope stays readable, and a reader's verdict is a property of the record
  rather than of the bus.
- `msgq-identity`: the bearer token authorizes a principal to the bus; the signature identifies a
  speaker to a peer. The requirement that a session cannot speak as a sibling session is narrowed to
  what the bus actually enforces. `msgq.register_identity` stays as the bus's authority.
- `msgq-cli`: `msgq keygen`, `msgq fingerprint`, `msgq trust` (add, revoke, rotate, list), a
  `--verify` or default verdict on read, and the `MSGQ_TRUST` / `MSGQ_INSECURE` environment
  variables. `db info` reports the trust posture and the layers in force.
- `msgq-extension`: `/msgq-trust` with `status`, durable pinning, and `--session`; the renderer
  names the verdict of every record and never hides an unverified one.

## Impact

- `cli/src/`: new key and keyring modules, signing on publish, verification on read, `MSGQ_TRUST`
  parsing, and `db info` reporting. `cli/src/pg/` is untouched by verification, because the
  reader verifies; the server-side write path keeps its scope check unchanged.
- `protocol/fixtures/`: signed records, a record with a bad signature, a record signed by a retired
  key, and a mixed stream of signed and unsigned records.
- `extension/`: a new command, a trust resolver bound to the session's single bus, and verdict
  rendering.
- A new dependency on `ed25519-dalek` (and `rand` for key generation) in the Rust CLI, and on a
  matching ed25519 implementation in the extension if the extension verifies rather than delegating
  to `msgq`.
- Operators of existing buses must either sign or set `MSGQ_INSECURE=1`; that is the breaking edge,
  and it is deliberate.
