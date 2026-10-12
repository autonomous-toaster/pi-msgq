## Why

The transport works and nothing can use it. The extension has zero tools, so an agent cannot
message a peer, start one, or delegate anything — the bus is a system with no users. Meanwhile
herdr already knows how to launch a real `pi`: it names agents, tracks their status, and reports
their working directory.

This change turns the transport into delegation. Two tools over the existing bus, one command
over herdr, and no new storage.

## What Changes

- Add `companion/`: a shell command that starts a companion in a new herdr tab. It deduplicates the
  name, runs a raw `pi` wired to the bus, passes the parent address both as an environment variable
  and as a pi argument, and blocks until the companion is reachable on the bus. It can also close a
  companion, and `--dry-run` prints the commands it would run.
- Add `msgq ask <address> [message]`: publish a message and wait for a reply. It ends on a reply,
  on a terminal acknowledgement, or on its timeout, and a timeout is not an error.
- Add `msgq publish --reply-to <offset>`: attach a reply reference to a published record.
- Add two extension tools: `peers` (list, send, ask, reply) and `companions` (spawn, close). They
  are thin adapters over the two commands above.
- Add `--msgq-report-to` (and `MSGQ_PARENT`) so a companion knows its parent, and `--msgq-trust` so
  under `honor` only allowlisted senders can start a turn.
- Discovery is split deliberately: `peers` reports who is reachable from the bus (portable, lean),
  `companions` reports where they are and what they are doing from herdr (rich, local-only).

Explicitly not in this change:

- No handover or context transfer.
- No orphan reaping; a companion that never finishes is left for the operator.
- No attachment payloads, no result payloads in acknowledgements, and no resumption by waking.
- No cross-machine companions.
- No in-process subagents: a companion is a real `pi` process in its own herdr tab.

## Capabilities

### New Capabilities

- `msgq-companions`: the `companion` command — spawning and closing companion agents in herdr
  tabs, wired to the bus.

### Modified Capabilities

- `msgq-cli`: add awaiting a reply (`ask`) and a reply reference on publish.
- `msgq-extension`: add the `peers` and `companions` tools, the reporting address, and a trusted
  sender allowlist on the delivery policy.

## Impact

- New top-level `companion/` deliverable, alongside `cli/` and `extension/`.
- The extension resolves two binaries the same way: `MSGQ_BIN` for the CLI and `MSGQ_COMPANION` for
  the companion command, each defaulting to its name on `PATH`; `MSGQ_SPAWN_EXTRA` injects extra
  pi arguments so a companion can be started from a working tree during development.
- Requires `herdr` (for spawning) and `jq` (for its JSON).
- No bus schema change, no protocol version bump, and no new record type. `ask` and replies are
  ordinary records carrying `reply_to`.
- A companion runs with `honor`, so the trusted sender allowlist is what keeps a message from
  commanding a shell-capable agent.
