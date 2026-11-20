# Design

See `proposal.md` for why. This document records how the trust layer is built, and the choices that
would be expensive to reverse.

## Context

Two facts about the current shape drive most of the design.

The bus is not a process. A local bus is a sqlite file that `msgq` opens per invocation; a shared bus
is Postgres evaluating pl/pgsql on somebody else's machine. Nothing else ever decides anything. So
"the bus verifies" has no meaning — verification can only happen in the client, and the only
question is which client.

The client already owns the vocabulary. `msgq-protocol` requires one place to define the record
vocabulary and one set of fixtures every implementation must pass, and `cli/src/record.rs` is where
that lives. A second implementation of the record format in TypeScript would be exactly the drift
the protocol spec forbids.

## Goals / Non-Goals

**Goals:**

- A verdict per record that a reader reaches without asking the bus.
- Trust established out of band, once, as a human act, and never inferred.
- Zero new ceremony for the single-uid local case beyond having a key.
- One implementation of signing and verification, checked against the existing fixtures.

**Non-Goals:**

- Proving a key belongs to a named human. A fingerprint identifies a key; who holds it is a social
  fact established by whatever channel the operator used.
- Session subkeys. One key per principal was chosen deliberately; a sibling session of one principal
  therefore remains cryptographically indistinguishable from its neighbour.
- Key recovery, profiles, a directory service, or revocation lists published anywhere.

## Decisions

### The signature lives in `meta.sig`, not in a top-level field

`msgq-protocol` guarantees a client preserves unknown `meta` keys and never fails on one, and it
declares `meta` open. A top-level field would be a new required-ish field on every reader. Putting
the signature in `meta` makes it an additive change that keeps `v: 1`, and it means a reader that
does not verify still accepts every signed record — which is what "additive change needs no bump"
was written for.

`meta.sig` is an object: `{ "alg": "ed25519", "key": "<base64 pubkey>", "sig": "<base64>" }`. The
fingerprint is derived from `key` rather than carried, so a record cannot claim one fingerprint and
present another key.

Alternative rejected: signing the whole record including `meta`. It would make the signature cover
fields the bus may legitimately restamp, and it would forbid ever adding a `meta` convention to a
signed record.

### The signed bytes are a domain-separated, length-prefixed encoding

What is signed:

```
  "msgq.sig.v1" || 0x00
  canonical(from) || 0x00
  canonical(to)   || 0x00
  type            || 0x00
  sha256(body as stored)
```

Every field is length-delimited or fixed-size, so no two distinct records produce one byte string.
`from` and `to` go through the existing canonicalization, which is already a solved problem and
already the thing `sender_of` checks, so the signed sender and the checked sender cannot disagree.
The body is hashed because a sealed body is opaque bytes and an event body is JSON; hashing makes
one rule cover both, and it keeps the signature input small.

The one thing this deliberately does not prevent: the bus can duplicate or drop a record. It can
also move a valid signature onto an identical `(from, to, type, body)` record, which is the same
record. Neither is impersonation, and a bus can always do both.

### Signing and verification happen in the Rust client, once

Verdicts are computed in `cli/src` during the read path and emitted with each record. The extension
renders them; it does not implement ed25519. This keeps one implementation of the wire format,
reuses the fixtures, and matches how the extension already reaches for `msgq` for everything that is
not delivery.

It also removes the obvious performance worry: verification happens inside the same `msgq read`
call that fetches the records, so a batch costs no extra process per record.

Cryptography comes from `ed25519-dalek`, with `rand` for key generation and `sha2` for the body
digest. No new protocol, no SSH wire format.

### Keys and anchors are two different things, with two different consent rules

This distinction carries most of the UX weight:

```
  YOUR KEY                          THEIR ANCHOR
  ----------                        -------------
  proves what you said              says what you believe about them
  created lazily on first publish   only ever written by an explicit act
  local file, 0600                  per-bus keyring
  reported once, then silent        human copies a fingerprint out of band
```

Your own key may appear without ceremony — generating it trusts nobody. An anchor may not: that is
the decision the whole design exists to keep human. Making this explicit is what lets the design be
strict by default without making a fresh local bus unusable.

### The keyring is scoped by bus, and the bus id reuses the naming discipline already in `pg/naming.rs`

`~/.config/msgq/trust/<bus-id>/anchors.toml`, where `bus-id` is a readable slug plus a digest of the
canonical bus location: the resolved absolute path for a file bus, `host:port/db` for a server bus.
That is the same shape `pg/naming.rs` already uses for queue names — slug for legibility, digest for
identity — so there is one idiom rather than two.

Anchors are per principal: `current` fingerprint, plus `retired` entries with the date they were
retired. Writes are atomic (temp file, then rename), because a half-written keyring would silently
change what a reader trusts.

### Three layers exist, and only two of them exist in the CLI

```
  layer       CLI                      extension
  ---------   ----------------------   ------------------------
  env         MSGQ_TRUST               MSGQ_TRUST (inherited)
  session     --                       in memory, per session
  durable     anchors.toml             anchors.toml

  precedence: env > session > durable
```

The session layer has no file and no representation in the CLI, which is the point: it must not
outlive the process that created it. The extension's `/msgq-trust --session` writes to memory only.
Disagreement between layers is reported and never resolved silently, because a silent resolution
would be the one place where trust changes without an act.

### Strict is a bus posture, checked on read, and `--insecure` is an environment variable

An unsigned record is refused on read unless `MSGQ_INSECURE=1`. "Insecure" was chosen over a positive
name like `MSGQ_REQUIRE_SIG=0` because it cannot be misread: there is no state in which setting a
variable named `INSECURE` makes a run more secure, so nobody can persuade themselves that `=0` is
the safe setting. It is per invocation and never persisted, which keeps "we will turn signing on
later" from silently becoming "we never did".

Refusal is on the read path rather than the write path on purpose: the bus stores what it is given,
history is never rewritten, and the strictness belongs to the reader who knows what it needs.

### Rotation retires, and the report pays for the scan

Rotating moves `current` to `retired` and writes the new fingerprint. Records signed by a retired key
are refused, which is the semantics that were chosen: rotation kills history. Because that is
destructive, rotation counts the stored records it affects and names the number before the operator
walks away. The scan is bounded by the retention window, which the bus already declares — so the
cost is bounded by a number the protocol spec already makes public.

## Risks / Trade-offs

**A lost key is a lost identity.** No recovery is in scope.

→ Mitigate by documentation and by the key file being a plain file an operator can back up; the
fingerprint they shared is the thing to re-pin if they restore it.

**Retired-key refusal makes old history unreadable.** That is the chosen semantics, but it can
surprise.

→ Rotation reports the count. A refused record is shown with its reason rather than silently
dropped, so history stays browsable even when it is not trusted. The encryption spec's precedent —
a reader preserves what it cannot open — is honoured.

**Strict default breaks every existing unsigned bus on upgrade.**

→ `MSGQ_INSECURE=1` is the documented escape hatch, and `db info` states the posture plainly so an
operator can see why a read failed. If this proves too sharp in practice, a warn-only release
before enforcement is the fallback; it is a one-line policy change because the check is already on
the read path.

**Auto-generating a key on first publish means the first run of a fresh local bus creates state.**

→ The notice names the fingerprint and where it was written, so it is reported rather than silent,
and it grants no trust to anyone.

**Signing does not fix the sibling-session case, and someone will assume it does.**

→ `msgq-identity` gains a requirement stating that one credential names one principal, not one
session, and that telling sessions apart needs a signature — which this change does not provide.
The overclaim in the `sender_of` comment is corrected as part of the implementation.

**Verifying a sealed body's storage bytes couples the signature to the encryption layer.**

→ Accepted: the signature covers what is stored, so a re-encryption invalidates it. That is
correct — a record whose bytes changed is not the record that was signed.

## Migration Plan

1. Ship `msgq keygen`, `msgq fingerprint`, and signing on publish, with reads refusing unsigned
   records unless `MSGQ_INSECURE=1`. Publish signs automatically once a key exists.
2. Operators with existing unsigned history set `MSGQ_INSECURE=1` until they re-key and re-publish,
   or accept that retained history reads as unverified-by-refusal.
3. Ship `msq trust` verbs and the keyring; pin peers from fingerprints exchanged out of band.
4. Ship `/msgq-trust` and verdict rendering in the extension.
5. Rollback: unset trust, unset `MSGQ_TRUST`, and set `MSGQ_INSECURE=1`. No stored record is ever
   rewritten, so the change is reversible by configuration alone.

## Open Questions

- The exact fixture byte vectors for the signature cases are best fixed against the implementation
  rather than guessed here; they belong with `protocol/fixtures/` and must be reproducible from a
  fixed key so both clients agree.
- Whether the OpenSSH `.pub` line should be accepted on input as well as our own fingerprint form
  is a convenience question for the trust verb; it does not change any requirement above.
