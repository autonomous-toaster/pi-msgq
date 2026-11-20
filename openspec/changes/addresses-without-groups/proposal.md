# Proposal

## Why

Three of the four address schemes name a *set* rather than a party: `group/<name>` and `cwd/<enc>` are
fan-out addresses, and a bare name is a shorthand for the first of them. A set cannot be sealed to,
cannot be addressed by a signature, and makes the shorthand mean the opposite of what everybody
expects — `msgq publish reviewer` writes to `group/reviewer` while `/peer reviewer` sends to
`session/reviewer`. Every one of those surprises traces back to a scheme that should not exist.

## What Changes

- The reserved schemes become `session`, `cli` and `bus`. `group/` and `cwd/` are removed: an address
  names one party, or it names the bus itself.
- A bare name SHALL be canonicalized to `session/<name>`, which is what the extension already does and
  what a person means.
- Presence moves to `bus/presence`, with `bus/<scope>/presence` for a scoped roster. That is the one
  address that names no party, and it is the bus's own front page.
- A presence record SHALL NOT be sealed. It is public metadata by design and must be readable by
  every participant, because building the roster and discovering a peer's agreement key both depend
  on it. It is signed, so it is trusted without being hidden.
- `msgq topics`, the fixtures, the extension's roster and every documented example are rewritten
  against the three remaining schemes.
- **BREAKING**: any existing stream named `group/...` or `cwd/...` becomes unaddressable. No migration
  is provided: the bus is read by the same release that writes it.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `msgq-cli`: the address grammar keeps three schemes and canonicalizes a bare name to a session; an
  address naming a set is gone.
- `msgq-protocol`: presence has one well-known address per scope under `bus/`, and that address is
  never sealed.
- `msgq-identity`: a scope names the addresses one principal may reach, and with no fan-out scheme
  left, a scope covers the addresses it names and no subtree.
- `msgq-extension`: the roster address and the peer listing follow `bus/presence`, and the extension
  publishes its presence there.

## Impact

- `cli/src/address.rs` (the grammar), `presence.rs` (the address), `commands.rs` and `store.rs` (the
  `group/` examples and the `queue_name` slug), `extension/src/config.ts` and `presence.ts`.
- `protocol/fixtures/*`: eleven fixtures name `group/...` or `cwd/...` and are rewritten; the
  expectations file keeps the same cases.
- Every test that uses `group/a` as a convenient address — the cheap address of choice in the suite —
  becomes `session/a`.
- Documentation: the grammar table in `cli/README.md`, the examples in `README.md`, and the presence
  section of `extension/README.md`.
- `recipient-only-sealing` builds on this: with no set-naming address left, every address is either a
  party to seal to or the roster to leave alone.
