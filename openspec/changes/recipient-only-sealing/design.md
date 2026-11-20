# Design

See `proposal.md` for why. This change deletes; the design is mostly about what must be true first.

## Context

`addresses-without-groups` leaves three schemes, so every address is either one party or the roster.
That is the precondition this change spends: with no set-naming address there is nothing a bus-wide key
could legitimately seal for, which is the only reason one existed.

## Goals / Non-Goals

**Goals:**

- One sealing scheme, one failure mode, one place a key comes from.
- A publish either reaches the party it names or fails saying why.
- Nothing in the environment can silently weaken what gets sealed.

**Non-Goals:**

- Reading anything the passphrase scheme wrote. Backward compatibility is not wanted here.
- Forward secrecy against the recipient's own key, for the reason the previous change gave: the queue
  must be readable after a restart.

## Decisions

### Delete the scheme, do not deprecate it

A deprecated second scheme is a second failure mode with a deprecation notice. Everything that made it
necessary is gone, so it goes: `cli/src/seal.rs`'s scrypt key, `MSGQ_PASSPHRASE`,
`MSGQ_PASSPHRASE_FILE`, `MSGQ_KEY_SALT`, the salt-derived key identifier, and the additional-data
version that existed only to keep its records openable.

`envelope.rs` then owns sealing outright: one algorithm tag, one declaration shape, one binding.

### An unknown recipient is an error, because it now means "nobody"

The fallback existed because a first contact had no key to seal to. With presence signed and published
on the roster, every live session advertises its agreement key, so a key that cannot be found means the
address is wrong or the session is gone. Both are worth an error, and neither is worth a silent
downgrade to plaintext.

Cost, stated plainly: publishing to a session that has *never* run — a typo, or a session about to
start — fails. That is the trade this change makes deliberately, and `--plaintext` is not there to
soften it.

### The roster is the one address that is not sealed, as a requirement

`addresses-without-groups` states it; this change depends on it. Presence is how a key is learned, so
sealing presence would make the roster unavailable to exactly the clients that need it.

### One binding, no compatibility path

The additional data binds the canonical sender and the address, under one version. There is no older
version to accept because nothing that used it survives.

## Risks / Trade-offs

**Existing deployments keep `MSGQ_PASSPHRASE` set and expect it to do something.**

→ It stops being read, so it becomes inert rather than silently weaker. `db info` no longer mentions a
bus key, and unsetting it is a one-line note in the migration plan.

**A first publish to a session that has not started yet fails.**

→ Deliberate. It names the address; the fix is to wait for the session or check the address.

**Six fixtures and several requirements disappear.**

→ They tested the scheme being deleted. The conformance expectations shrink with them, and the fixture
count is asserted so the deletion cannot silently leave orphans.

## Migration Plan

1. `addresses-without-groups` lands first: it removes the scheme that the bus key was for.
2. Delete the passphrase scheme, its flags and its identifier; make an unknown recipient an error.
3. Republish anything that matters, or prune it: bodies sealed under the removed scheme are unreadable
   by design.
4. Rollback: revert the change. Records sealed under the new scheme stay readable, because the key it
   uses is the seed that survives either way.
