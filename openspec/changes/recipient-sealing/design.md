# Design

See `proposal.md` for why. This records how, and the choices that would be expensive to reverse.

## Context

Two facts shape everything.

The bus is not part of the key exchange. It stores what it is given and stamps a sender; it never
holds key material and never opens a body. So every decision below is about two clients agreeing
something the bus cannot see.

The existing key file holds a 32-byte ed25519 **seed** (`cli/src/keys.rs`), and `ed25519-dalek` already
pulls `curve25519-dalek`. Nothing needs to be added to the key store to gain a second key purpose:
the seed is a root, not a signing key that happens to be stored.

## Goals / Non-Goals

**Goals:**

- A record readable by its recipient and its sender, and by nobody else on the bus.
- One file, one fingerprint, one ceremony per principal.
- Old records, plaintext records and new records on one stream, each read by its own rules.
- One implementation of the encoding.

**Non-Goals:**

- Group membership. A group address keeps the bus key and says so.
- Forward secrecy against the recipient's own key: the recipient must read its backlog after a
  restart, so its key outlives the retention window by construction.
- Hiding who talks to whom. The envelope stays readable; that is a property, not an oversight.
- Multi-device, or a key held anywhere but `~/.config/msgq/keys`.

## Decisions

### The seed derives two keys, and neither is the seed

```
  seed (32B, ~/.config/msgq/keys/<principal>.key)
    |
    +-- as stored                        -> ed25519 seed   (what keys.rs already uses)
    +-- HKDF-SHA256(info="msgq/agree/v1") -> X25519 scalar, clamped
```

**The signing key is not re-derived.** A file already on disk holds the ed25519 seed verbatim, and
signatures already stored must keep verifying, so that derivation cannot change. The HKDF step applies
only to the agreement key, which is why it carries a domain-separated label and the signing key does
not.

The honest statement of the compromise: the two keys are independent — one is the seed, the other is a
hash of it, so neither reveals the other and no scalar serves two purposes — but a leak of the file
leaks both. That was already true of the file, and separating them still buys what matters here: a
runtime compromise of one purpose does not hand over the other, and rotating one does not force the
other.

Alternative rejected: reusing the ed25519 scalar for X25519 by birational map (what libsodium's
`sk_to_curve25519` does). It is defensible, but it makes one scalar do two jobs, and the cost of
avoiding it here is one HKDF call.

### The agreement key travels in `meta.sig.agree`, inside the signed bytes

```json
{ "meta": { "sig": { "alg": "ed25519", "key": "...", "agree": "<base64 x25519 public>", "sig": "..." } } }
```

- No directory, no card, no claim stream: a key is found on any record the principal ever wrote.
- Substituting it breaks the signature, so it inherits the trust verdict of the record it rides on.
- Works offline, which a presence card cannot, and survives the sender's process exiting.

The cost is ~44 bytes per signed record and one rule for clients: to seal to somebody you must have
one of their records. That is a real limit and the error message says so.

### The body is sealed to a content key, and the content key to each recipient

```
  cek      <- 32 random bytes, per record
  body     <- AES-256-GCM(cek, plaintext, nonce_body, aad_v2(record))
  per recipient r:
     eph   <- random X25519 scalar, per (record, r)
     shared<- X25519(eph, r.agreement_public)
     kek   <- HKDF-SHA256(shared, salt=eph_public||r_public, info="msgq/seal/v1")
     box   <- AES-256-GCM(kek, cek, nonce_kek, aad_v2(record))
  meta.enc = { alg: "x25519+aes-256-gcm", aad: "v2", nonce: <body nonce>,
               cek: [ { to: "<fp>", epk: "<b64>", box: "<b64>", nonce: "<b64>" }, ... ] }
```

- One body, N small wraps, so a record does not grow with its audience beyond ~124 bytes per reader.
- The ephemeral is per **recipient**, not per record: two recipients must not be able to confirm
  they received the same ciphertext by comparing one public value.
- The salt binding `eph_public || recipient_public` makes the derived key unique to that pair even if
  an ephemeral were ever reused by a bug.
- The KEK and the body use different nonces and the same AAD, so a wrap cannot be moved between
  records, addresses or senders.

### The additional data gains the route, under a version

```
  aad v1 (existing):  "msgq/<v>/<type>"
  aad v2 (this change): "msgq/<v>/<type>\0" + canonical(from) + "\0" + to
```

`meta.enc.aad` selects it; an absent field means v1, which is what every record sealed so far
carries. Nothing stored is rewritten, and a v1 body still opens.

The v1 form bound only the type because the bus stamped the sender *after* the client sealed. That
reasoning is gone: the sender is inside the signed bytes and is verified before the body is opened, so
v2 can bind the route without making a record unopenable.

### The key identifier is superseded, not fixed

The existing `kid` is `SHA256(salt)` (`seal.rs`), which identifies the derivation input rather than the
key — it is the same for every deployment using the default salt, so it cannot tell "wrong key" from
"right key, wrong record". The encryption spec asks for a key identifier derived from the key, so this
is a defect.

It is not fixed in place. A new algorithm tag carries `cek[].to`, the recipient's fingerprint, which is
a real identity; the old tag keeps its old identifier, because records sealed under it must open
exactly as they did. A reader switches on `alg`, which it already does.

### Sealing is a rule about the address, and the escape has to be typed

```
  session/…, cli/…      -> seal to the recipient and the sender (error if the key is unknown)
  group/…               -> bus key if configured, else plaintext; reported either way
  --seal-to <address>   -> that recipient and the sender, whatever the address kind
  --plaintext           -> store unsealed, and say so
```

The error on an unknown recipient is the point: a first-contact message on a shared bus cannot be
sealed, and silently storing it in plaintext is the exact outcome this change exists to prevent.

### Opening is one verb, and the extension delegates

`msgq open` reads records on stdin and writes one outcome per record, mirroring `msgq verify`:

```
  { "opened": { ...record... } }
  { "sealed_to": "session/alice-1" }
  { "cannot_open": { "alg": "...", "reason": "..." } }
```

The extension calls it once per record that declares an encoding, and keeps no cryptography of its
own: `extension/src/seal.ts` loses both halves, its sealing half being dead already (the extension
publishes through `msgq publish`).

Cost: one short-lived process per sealed record. Plaintext records, which are the common case on a
local bus, cost nothing.

### Rotation archives the seed

`keygen --force` (or `--rotate`) writes `<principal>.<date>.key.retired` beside the key and says where.
Nothing is deleted, because a record sealed under the old key stays sealed under it forever: retiring a
decryption key is deletion, not policy, which is the mirror image of signature rotation refusing
history.

`msgq trust rotate` is untouched: it rotates what you believe about *somebody else*.

## Risks / Trade-offs

**A first-contact message cannot be sealed.**

→ It fails with an error naming the address and `--plaintext`. The alternative — quietly storing
plaintext — is worse than the friction.

**The agreement key is only as trustworthy as the record that carried it.**

→ Intended: it inherits the verdict. A stranger's key is `unverified`, and the reader is told so when
it matters.

**X25519 may not resolve offline.**

→ `x25519-dalek` is not in the local registry cache. Try `cargo add --offline` first; if it fails,
implement RFC 7748 over the `curve25519-dalek` already in the tree, gated by the RFC's own test vectors
and the all-zero-output check. A wrong implementation must fail a test, not a message.

**A group address is not protected per recipient, and somebody will assume it is.**

→ The client says which key it used, every time, and `db info` names the mode.

**Two keys from one seed means a seed leak is both.**

→ Already true of the file. The point of separating them is that a *runtime* compromise of one purpose
does not hand over the other, and that rotation of one does not force the other.

## Migration Plan

1. `msgq open` ships first and is a no-op for records it can already open: the encoding gains a reader
   before it gains a writer.
2. The agreement key appears in `meta.sig.agree`; readers that do not know it preserve it, as with any
   additive `meta` key.
3. Sealing by recipient is opt-in per address kind at first (`--seal-to`), then becomes the rule once
   `open` is in place on both clients.
4. Rollback: unset the passphrase and pass `--plaintext`; nothing stored is rewritten, and records
   already sealed stay readable by whoever holds the keys.

## Open Questions

- Whether an explicit `--seal-to` should be allowed to name a group's members as a list, which is the
  smallest honest step toward group encryption without inventing membership. Deferred: it is a new
  surface and this change does not need it.
