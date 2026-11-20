# Proposal

## Why

Two sealing schemes exist: an envelope addressed to recipients, and a passphrase-derived key that
belongs to the bus, so that everyone holding it — the operator of a shared server included — reads
everything. The second one exists for a set of addresses that `addresses-without-groups` removes, and for
records written before the first one existed. Neither reason survives: there are no set-naming
addresses left, and nothing here needs to read what an older release wrote.

It is also the scheme that bit its own user: an ambient `MSGQ_PASSPHRASE` silently applied a weaker key to
a session record that had no known recipient, which is a downgrade nobody asked for and nobody saw.

## What Changes

- One sealing scheme: a body is either sealed to its recipients or it is not sealed at all.
- **BREAKING**: the passphrase scheme is deleted — `MSGQ_PASSPHRASE`, `MSGQ_PASSPHRASE_FILE`,
  `MSGQ_KEY_SALT`, the scrypt derivation, the bus-key identifier, and any ability to open a record
  sealed under them.
- **BREAKING**: a publish to a session or `cli` address whose agreement key is unknown is the error it
  always should have been. There is no fallback to a bus key and no `--plaintext`.
- The additional-data version becomes the only one: a body is bound to its canonical sender and
  address, and the older binding is no longer accepted.
- `db info` stops reporting a bus key and reports the one scheme, with the agreement key in force.
- The roster address is never sealed, which `addresses-without-groups` puts in writing.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `msgq-encryption`: sealing is recipient-only. The requirement that a key comes from custody, the
  mixed-history requirement, and the key-identifier requirement are replaced by one scheme and one
  failure mode.
- `msgq-cli`: `publish` loses `--plaintext` and the fallback; `db info` loses the bus-key line.
- `msgq-protocol`: one additional-data binding, no key identifier, and no declaration for a scheme
  this build no longer implements.
- `msgq-extension`: sealing is what the client does; the extension keeps no cryptography at all, and a
  record sealed to somebody else is reported rather than raised as an error.

## Impact

- Deletes `cli/src/seal.rs`'s passphrase half (the scrypt key, the `kid`, the v1 additional data) and
  `extension/src/seal.ts` with it; `envelope.rs` keeps the whole job.
- Removes six fixtures that exercise the passphrase scheme, and the "sealed body", "mixed history" and
  "opaque body" cases that depend on it; the conformance expectations shrink accordingly.
- `msgq-encryption` goes from six requirements to four, and `PROTOCOL.md` loses the key-identifier
  paragraph.
- Any deployment with `MSGQ_PASSPHRASE` set should unset it: it stops being read, and the variable
  becomes inert rather than silently effective.

## Depends on

`addresses-without-groups`: with no set-naming address there is nothing left that a bus key could
legitimately seal for.
