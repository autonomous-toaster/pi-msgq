## Why

A session id is a poor discovery key: an agent that wants a reviewer in the current project has to
know the id before it can ask. Presence already says who is live, but not where that peer is
working. A small locator card on the presence record lets peers be found by project, branch,
worktree, or path instead.

## What Changes

- Add an optional flat `event.card` object to presence records with four locator keys: `project`,
  `branch`, `worktree`, and `path`. The protocol stays at `v: 1`; readers that do not know the field
  keep working.
- Add `msgq card`, which derives the locator from the current directory and git.
- Carry the card through `msgq sessions --json`, and show it in human output only with a new
  `--wide` flag. Default text output stays unchanged.
- Have the pi extension and the maki plugin derive the card, publish it with presence, refresh it
  when the working directory changes, and expose a narrow `card` action limited to the four locator
  fields.
- Document the field semantics, path modes, privacy implications, and the 512-byte cap.

Explicitly not in this change:

- No A2A AgentCard shape. A2A was reviewed and rejected for this scope: its fields describe an
  HTTP/gRPC service, while msgq peers are local bus addresses.
- No profile fields, free-form metadata, capabilities, current task, progress, or work summary.
- No `head`, `vcs`, or nested `where` object.
- No dedicated peer-search command; `msgq sessions --json` plus the card is enough for this
  iteration.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `msgq-protocol`: presence gains an optional locator `card` convention, with additive compatibility
  and defined field semantics.
- `msgq-cli`: adds `msgq card`, parses the card in the roster, adds it to `sessions --json`, and adds
  `sessions --wide`.
- `msgq-extension`: derives, publishes, refreshes, and displays the card; adds a narrow `card` tool
  action and configuration for context derivation.

## Impact

- The wire gains one optional event field; no protocol version bump and no new record type.
- The CLI gains one subcommand and one `sessions` flag.
- The pi extension and maki plugin gain a card field, a small derivation helper, and a card action.
- Presence renewals become slightly larger: four short fields per participant. The card is capped
  at 512 bytes and remains optional.
- Presence is public metadata. Project names, branch names, worktree names, and paths can leak
  private repository or directory information; relative paths are the default and absolute paths
  are opt-in.
- No new dependency. Derivation uses the existing `git` binary and fails soft when it is absent.
