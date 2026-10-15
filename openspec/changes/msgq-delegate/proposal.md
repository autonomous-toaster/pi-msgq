## Why

The tools exist and the strategy does not. An agent has `peers` and `companions`, and can see their
descriptions, but nothing tells it when delegating is worth a whole second agent, how to write a
task a fresh process can act on, or which of its instincts will fail silently.

Worse, an agent cannot see **itself**. A companion knows its depth only by being refused when it
tries to delegate, cannot read the cap that refused it, does not know which senders it is allowed to
obey, and cannot tell which address on the roster is its own. The human has `/peers me` for exactly
these facts. The model has nothing.

And the topology is a star whether or not that is what was wanted: a spawned companion's trusted
sender set is hardcoded to its parent, so two delegates cannot act on each other.

## What Changes

- Ship a second skill, `msgq-delegate`, teaching the agent's half of collaboration: delegating work
  to another session, writing a task that survives an empty context, when to block and when to be
  woken, running several delegates, and the traps that fail silently.
- Let the model see itself: the `peers` tool's `list` leads with this session's own facts — its
  address, delivery policy, what it follows, which senders it trusts, the recipient it defaults to,
  and how deep it is.
- Make depth legible rather than a surprise: the extension reads the depth it was started with, and
  `companion spawn` passes the cap along with it, so a companion can answer "may I delegate?" before
  trying.
- Let the caller widen the trusted set: `companion spawn --trust <addresses>`, defaulting to the
  parent alone. This is what makes anything other than a star reachable.

Explicitly not in this change:

- No mesh protocol, no relay, and no addressing of siblings. A trusted set is fixed at spawn,
  because the addresses of one's siblings may not exist yet; a set that is updated at runtime is a
  new mechanism with its own failure modes. Two delegates that must confer go through whoever
  started them.
- No group messaging, and no "speak as the family" option. A peer answers as itself; `--from` is the
  one field it must not fake.
- No handover or context transfer, still: the task message is written by hand.
- No spawn-with-task. Spawning creates a process; the task is the first message, as before.
- No reaping, and no record of which companions this session started.
- No change to the four human commands, to `/peers me`, or to the wire.

## Capabilities

### New Capabilities

None. Every requirement lands on the capability that already owns the behaviour: the extension owns
what the model is told, and the companion command owns how an agent is started.

### Modified Capabilities

- `msgq-extension`: report this session's own identity to the model, and make the shipped-skill
  requirement cover however many skills are shipped rather than the one it names today.
- `msgq-companions`: state the trusted sender set, and let the caller widen it; and tell a companion
  the cap as well as its depth.

## Impact

- A second skill directory under `extension/skills/`, declared by the same `pi` manifest. Loading the
  extension is still what makes both available, so a companion inherits them too.
- `peers list` stops being a plain passthrough of `msgq sessions`: it gains a header from the
  extension's own state. Nothing about the roster itself changes.
- `companion spawn` gains one option and passes one more environment variable into the tab.
- The claim "a companion answers its parent and nobody else" becomes a **default** rather than a
  property. It is worth being explicit about why that is not a security change: `from` is
  unverified, so the trusted set was never an authentication boundary. It scopes what a
  shell-capable agent will *routinely* obey, and the real boundary is who can write the bus file.
