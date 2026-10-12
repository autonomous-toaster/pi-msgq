## Why

The bus has two users and one of them is typing. An agent can list peers and send messages; the
human at the keyboard can only ask the agent to do it and pay for the round trip, or leave the
session for a shell, where the session's own identity is something they have to retype. The roster
is readable, the address is known, and the person who owns the bus cannot speak on it.

## What Changes

- Add four extension commands, for the human: `/peers` (the roster, then pick a peer and compose),
  `/peer <address> [message]` (send), `/reply <message>` (answer the last inbound), and
  `/companions` (the herdr-side view).
- `me` is an address alias everywhere. `/peers me` prints this session's identity — address, bus,
  topics, policy, presence, trusted senders — and `/peer me` publishes to yourself.
- A bare name resolves to `session/<name>`, so `/peer reviewer "..."` works without the prefix.
- A send with no target opens the same picker `/peers` uses; a send with no message opens the
  editor.
- Every command is fire-and-forget. Nothing waits for a reply, because a reply is an inbound
  message and that path already exists.
- The message renderer gains a human-facing reply affordance — presentation only, never part of
  the content the model reads.
- Add `companion list`: the herdr side of the roster, joining `herdr agent list` to the bus by the
  name that is already shared across herdr, the pi session, and the address.

Explicitly not in this change:

- No overlay, no keybinding, no live roster pane. Commands only.
- No waiting, no `--wait`, no timeout UX.
- No terminal bell and no desktop notification on arrival.
- No `meta.origin` to mark a message as human-sent. It needs a `publish --meta` flag, and it would
  be a hint rather than a guarantee, since any client can write the field.
- No roster cache. If address autocompletion is wanted later, it must not shell out per keystroke.

## Capabilities

### New Capabilities

None. The commands live in the extension, are registered in the extension's lifecycle, and speak
with the extension's identity, so their requirements belong to the extension. `companion list`
extends a command that already owns herdr.

### Modified Capabilities

- `msgq-extension`: add the four human commands, the `me` alias and bare-name resolution, the
  shared peer picker, and a reply affordance in the renderer.
- `msgq-companions`: add `companion list`, the herdr view of the roster.

## Impact

- No new deliverable. The commands are a new module in `extension/`; `companion/` gains one verb.
- No bus schema change, no protocol version bump, no new record type, and no CLI change: every
  command is a call to `msgq publish` or `msgq sessions`, and both exist.
- `/companions` needs `herdr`, so it degrades where the rest does not. A session outside herdr
  still has `/peers`, `/peer`, and `/reply`.
- Commands exist only where a human is driving. `json` and `print` have no dialogs, so the picker
  and the editor are unavailable there.
- The human and their agent share one identity, `session/<self>`, so a companion that trusts the
  session answers both.
