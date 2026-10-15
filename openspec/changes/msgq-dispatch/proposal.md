## Why

A slash command is an affordance for the person at the keyboard, and the bus has no way to invoke
one. So when an agent asks a peer to run `/harsh-review`, it can only describe the command — and the
peer does the only thing left to it: it goes looking for the command's definition on disk and tries
to follow it by hand. That is not a workaround failing; it is a model being asked to do something it
cannot do.

The primitive is not missing. The extension delivers every record with `pi.sendMessage`, which
produces a custom transcript entry — content. A record can also be delivered with
`pi.sendUserMessage({ expandPromptTemplates: true })`, which goes through the same path a **typed**
prompt does: extension commands run, `/skill:` bodies load, prompt templates expand with their
arguments. The bus has been carrying descriptions of actions and never actions, and only one of the
two delivery modes is in use.

## What Changes

- Add a record type, `command`. A command record's body is a command line, invoked by the receiving
  session exactly as if it had been typed. `msgq publish <address> "/harsh-review review the parser"
  --type command`.
- Deliver it as input rather than as content: the extension sends the body through the dispatcher,
  so a template expands, a skill loads, an extension command runs, and anything unrecognized passes
  through as ordinary text instead of failing.
- Keep the provenance that content delivery has today, by sending a note **before** the command:
  one entry naming the sender and the address, then the command itself. A command must be the whole
  text to be a command, so this is the only place the sender can appear.
- Let the model mean it: the `peers` tool gains a way to send a command, and refuses to ask one,
  because a question is wrapped in a reply instruction that a command cannot carry.
- Teach the answer: a command cannot carry a reply request either, but prompt templates take
  arguments, so the sender writes the task and the request for an answer into the command line.
  That is knowledge for the skill, not mechanism.

Explicitly not in this change:

- No separate gate for extension commands. A dispatched command runs whatever a typed one runs,
  behind the same trusted-sender check that already gates a turn. That the check becomes far more
  load-bearing is written down as a risk, not engineered away.
- No reply mechanism for commands. `ask` refuses one rather than wrapping it in a hint that cannot
  be delivered.
- No change to how any other record type is delivered.
- No protocol version bump: the envelope is unchanged, and consumers are already required to
  tolerate unknown types.
- No way yet for a human to mean a command from `/peer`, which is left as an open question.

## Capabilities

### New Capabilities

None. The record type belongs to the capability that owns the record contract, and the delivery
belongs to the capability that owns delivery.

### Modified Capabilities

- `msgq-cli`: add `command` to the record contract's types, and define what a command record is.
- `msgq-extension`: deliver a command record as dispatched input while keeping its provenance, and
  let the model send one.

## Impact

- No new dependency, no new process, and no change to `companion`: an agent needs nothing more than
  the bus to be handed a command.
- A peer too old to know the type degrades safely — an unknown type is display-only — where a flag
  on a `prompt` would reproduce today's failure exactly.
- The delivery path gains a second mode, and one rule that follows from it: a command is never
  coalesced, because a batch is delivered as a single custom message and would stop being a command.
- Two sharp edges are accepted knowingly: a bus record can now run an extension command handler,
  which is code, without a model in the loop; and `/skill:x` bypasses `disable-model-invocation`,
  which exists to keep a skill in human hands.
