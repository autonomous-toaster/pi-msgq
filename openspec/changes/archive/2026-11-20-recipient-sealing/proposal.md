# Proposal

## Why

A body is sealed under one key derived from one passphrase, which makes it the bus's key rather than a
person's: whoever holds the passphrase reads every record, and on a shared server that includes
whoever operates it. The trust layer answered "who sent this"; this answers "who may read it", and
today the answer is "everybody", including the party the operator's threat model distrusts.

## What Changes

- A principal's existing key seed becomes the root of two derived keys: the ed25519 key that signs
  (unchanged) and an X25519 key that agrees. One file, one fingerprint to compare, no cross-protocol
  reuse of a scalar.
- Every signed record carries its sender's agreement key in `meta.sig.agree`, inside the bytes the
  signature covers. That replaces a key directory: a key is found from any past record, no new stream,
  and it works when the peer is offline.
- A sealed body becomes an envelope: one body sealed with a random content key, and that key wrapped
  once per recipient, each wrap under its own ephemeral X25519. A record is sealed to its recipient and
  to its sender, so a session can still read what it sent.
- The record declares it in `meta.enc`: the algorithm, the body nonce, and one entry per recipient
  naming that recipient's fingerprint.
- **BREAKING** for one verb: opening moves to a new `msgq open`, the sibling of `msgq verify`, and the
  extension delegates to it. The extension stops carrying any cryptography it does not need.
- Sealing is automatic for `session/` and `cli/` addresses. A recipient whose key is unknown is an
  error naming it, with `--plaintext` as the escape that has to be typed. A `group/` address keeps the
  bus key when one is configured, and says which of the two happened.
- The additional data a body is bound to gains the canonical sender and address, selected by
  `meta.enc.aad`; records sealed before this change keep opening exactly as they do today.
- The key identifier is superseded for the new algorithm by the recipients' own fingerprints. (The
  existing identifier is derived from the salt rather than the key, which contradicts
  `msgq-encryption` T1.1; new records no longer rely on it, old ones keep it, and no stored record is
  rewritten.)
- Rotating a key archives the seed it replaces, because retiring a decryption key is deletion rather
  than policy: signing rotation refuses history, and encryption rotation has to keep it readable.

## Capabilities

### New Capabilities

None. Every behaviour here belongs to capabilities that already exist.

### Modified Capabilities

- `msgq-encryption`: sealing stops being one key for the bus and becomes an envelope addressed to
  recipients; the key comes from the principal's seed rather than only from a passphrase; the key
  identifier no longer identifies a key; a reader distinguishes "not addressed to me" from "tampered
  with"; rotation archives rather than retires.
- `msgq-protocol`: `meta.sig` carries the sender's agreement key; `meta.enc` gains the recipient list,
  the body nonce, the algorithm and the additional-data version; a reader that cannot open a record
  reports that rather than failing the read.
- `msgq-cli`: `publish` seals to a recipient by rule, `open` is added, `keygen --rotate` archives the
  seed, and `db info` reports the sealing mode and the agreement key in force.
- `msgq-trust`: the agreement key is public material that a trust act may cover, and the fingerprint a
  human compares is the one that now certifies both purposes.

## Impact

- `cli/src/`: `keys.rs` gains the derivation of the agreement key; `seal.rs` gains the envelope and the
  additional-data version; `commands.rs` gains sealing rules and `open`; `trust/` reports and archives
  key material on rotation.
- `extension/`: `seal.ts` loses its sealing half and its opening half becomes a call to `msgq open`;
  the delivery path distinguishes a record sealed to somebody else from one that failed to open.
- `protocol/fixtures/`: RFC 7748 vectors for the key agreement, an envelope with two recipients, a wrap
  that does not open, a record sealed to a third party, and a mixed stream of old and new seals.
- A dependency on an X25519 implementation: `x25519-dalek` if it resolves offline, otherwise RFC 7748
  arithmetic on the `curve25519-dalek` already in the tree, gated by the vectors.
- Operators who seal nothing today are unaffected: a bus with no passphrase and no recipient key still
  stores plaintext, and old records are readable exactly as before.
