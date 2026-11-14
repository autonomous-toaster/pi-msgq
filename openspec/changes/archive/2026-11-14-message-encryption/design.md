# Design

## Context

Both engines store message bodies in the clear. On the local engine the only protection is the
permissions of the directory that holds the file, and permissions do not travel with a copy. On the
shared engine the protection is the scopes of the caller, which says nothing about a copy of the
database or about the operator of the server.

The protocol already reserves `meta.enc` for this, and both clients already refuse to interpret a body
that declares an encoding the client cannot honour. Encrypting a body is therefore a change above the
store, not inside it: the store sees a string either way, so the local and shared engines stay
behaviourally identical.

## Threat

The copy that matters is the file leaving the machine or the server: a backup, an external drive, a sync
service, a copy taken by hand. The defence is that a copy holds ciphertext and no key. A hostile process
running as the same user is not in scope: that process can read whatever the client can read, so no
client-side defence changes the outcome.

## Decisions

### 1. One key per principal, not per recipient

The unit of secrecy is the principal, because that is the unit of identity and of scopes. Per-recipient
encryption would defend session against session, which the threat model does not ask for, and it would
add key distribution and recipient metadata for no gain on a bus where one user owns every address.

### 2. Passphrase-first custody

A key derived from a passphrase is the only custody that survives a copy of the whole home directory:
the derivation input is never stored, so a copy of the key file next to the bus would be the same
failure as a plaintext body. An operating-system keychain is offered as an alternative and its limit is
stated: a backup of the keychain travels with a backup of the machine. A key file is offered as an
explicit escape hatch and documented as "the protection of the file is the operator's problem".

### 3. An authenticated cipher, and why it also buys integrity

The body is sealed with an AEAD construction and a fresh nonce per record, and the metadata names the
algorithm and the key identifier. A body altered in place fails to open instead of being read as
different content, so the same mechanism that hides the body also detects tampering with the body.

### 4. What stays in the clear

| stays readable | why |
| --- | --- |
| envelope: version, type, sender, recipient, reply reference | routing, delivery policy and acknowledgement matching depend on these |
| presence bodies | the roster is public metadata by convention |
| acknowledgement bodies | the acknowledgement lifecycle is public metadata by convention |
| addresses, offsets, timestamps, sizes | retention, gap accounting and cursors are keyed on addresses |

Encrypting these would mean re-architecting the bus around encrypted metadata. The residue is stated
rather than hidden: an adversary holding a copy learns who talks to whom, when, and how much, but not
what was said.

### 5. A mixed history is normal

Sealing is opt-in per record. A bus gains a key, records sealed under that key appear, and older records
stay plaintext. A reader must therefore handle both in one stream, and a reader that cannot open one
record must not fail the stream. This is why the change is additive and needs no migration.

### 6. Rotation is forward-only

The key identifier lets more than one key exist, so publishing with a new key while the old key stays
available for reading is the rotation path. Records sealed under the old key stay under the old key, so
a compromised key exposes what it already covered. The design states that instead of implying that
rotation reaches backwards.

### 7. The engine holds no key

Neither engine derives a key, sees a key, or re-encrypts anything. The shared engine stores ciphertext in
a queue and the local engine stores ciphertext in a file, which is also why the two engines need no
separate work and why a sealed record can travel between them.

## Risks

- A lost passphrase is unrecoverable by design; the design has no escrow and says so.
- Key derivation with a slow function costs latency on every command; the design keeps one derivation per
  invocation and notes the cost rather than caching a derived key on disk.
- Metadata remains visible, which will disappoint anyone who reads "encrypted" as "unobservable".
- The local engine's transcript is out of scope: the client that displays a body stores what it
  displayed, and msgq's responsibility ends at the record.

## Alternatives rejected

- Whole-file encryption: both clients read the file through a stock SQLite binding, so a per-file cipher
  would mean changing the binding for the Rust CLI and the Node extension at once, and it would still
  leave the shared engine unprotected.
- Compression before sealing: smaller bodies, but it leaks length more faithfully than the plaintext does.
