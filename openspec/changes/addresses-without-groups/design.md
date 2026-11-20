# Design

See `proposal.md` for why. This is a subtraction, so most of the work is deciding what replaces what.

## Context

Four schemes existed because four things were addressable: one session, one directory's sessions, a
named set, and a non-pi producer. Two of the four name a set, and a set is the thing that could never be
signed to, sealed to, or meant by a person. The roster, meanwhile, needs one address that is deliberately
nobody's.

## Goals / Non-Goals

**Goals:**

- Every address names one party, except the roster, which names the bus.
- A bare name means what a person means: one session.
- Presence is readable without a key, because the roster is how a key is discovered.

**Non-Goals:**

- Fan-out delivery of any kind. A message to several people is several messages, and that is a separate
  change if it is ever wanted.
- Migration of existing `group/` or `cwd/` streams.

## Decisions

### Three schemes, and one of them is the bus

```
  session/<id>      one pi session                       signed, sealed to it
  cli/<who>         one non-pi producer                  signed, sealed to it
  bus/presence      the bus's own front page             signed, never sealed
  <bare-name>       -> session/<name>                    the shorthand a person means
```

`bus` exists for exactly one reason: the roster is not a party and cannot be one. Naming it after the
thing it is keeps the grammar honest — every other word in the scheme position names an addressable
party, and this one says "the bus itself".

Alternative rejected: putting presence on `session/<id>` of some designated session. That makes the
roster depend on one participant being alive, which is the opposite of what it is for.

### A bare name is a session, and that is the whole point

`msgq publish reviewer` and `/peer reviewer` must mean one thing. The extension already resolves a bare
name to a session; the CLI resolved it to `group/`, which is why publishing to a peer by name quietly
went somewhere else.

### Presence is never sealed, and that is a requirement rather than an omission

Every participant reads the roster, and a participant that has never met anybody has no way to obtain
a key to seal to it. Encrypting presence would make the roster unavailable to exactly the clients that
need it, to protect metadata the protocol already calls public — project, branch, worktree, path.

The signature is what makes that safe: a presence record is trusted, not hidden, and its `meta.sig.agree`
is how a peer's agreement key is found. That is also what makes "every session has a key" true in
practice, which `recipient-only-sealing` depends on.

### Scopes become exact

A scope covered a subtree because `group/red` covered `group/red/sub`. With no set-naming scheme there
are no subtrees worth covering, so a scope covers the one address it names. That is a simplification of
`msgq-identity`'s `in_scope`, not a new capability.

## Risks / Trade-offs

**Existing streams named `group/...` become unaddressable.**

→ Deliberate, and cheap: the same release that stops writing them stops reading them, and there is no
second implementation to keep compatible.

**Someone used `cwd/` to reach a whole working directory.**

→ That is a fan-out, and it is gone. The roster still lists who is in a directory, so the human path
survives even though the address does not.

**A typo in a bare name now writes to a session instead of a group.**

→ Which is the point: it lands where the person meant, and a session nobody follows is the visible
failure rather than a group nobody follows.

## Migration Plan

1. The grammar lands first; `group/` and `cwd/` become usage errors naming the schemes that exist.
2. Presence moves to `bus/presence`; a scoped roster to `bus/<scope>/presence`.
3. Fixtures, tests and documentation are rewritten in the same change, because half a grammar is worse
   than either whole one.
4. Rollback: revert the change. Nothing on disk is rewritten, and the abandoned streams are simply
   never read again.
