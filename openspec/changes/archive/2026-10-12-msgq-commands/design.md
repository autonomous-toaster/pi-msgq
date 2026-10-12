## Context

See `proposal.md` for motivation and the two delta specs for the contracts.

What already exists, and what it constrains:

- The extension already plans and runs commands for the model's tools. `planPeerAction` and
  `planCompanionAction` in `extension/src/tools.ts` are **pure**: they take parameters plus a small
  context and return `{command, args}` or a reason it cannot be built. The extension runs the
  result. They are unit-tested without a bus, a peer, or herdr.
- The extension already holds everything a command needs about **this** session: its address
  (`self`), the bus location, the subscribed addresses, the delivery policy, the trusted sender
  set, the presence handle, and `lastInbound` (`{from, offset}`). It holds **no peer roster**: the
  only `msgq sessions` call in the extension is the `list` plan, run on demand when the model asks.
- `msgq publish` and `msgq sessions` already exist. **No CLI verb is needed for this change.**
- The spec's *Scoped write exception* says the extension writes presence leases and acknowledgements
  only. That is a live constraint on how a command is allowed to send.
- pi commands register with `pi.registerCommand(name, {description, getArgumentCompletions,
  handler})`. The handler receives `ctx.ui` (`select`, `input`, `editor`, `confirm`, `notify`,
  `custom`) and `ctx.hasUI`; `json` and `print` have no dialogs.
- `herdr agent list` reports, per agent: `name` when it has one, `agent_status`
  (`idle|working|blocked|done`), `cwd`, `foreground_cwd`, `tab_id`, `pane_id`, and
  `agent_session.value` — the pi session file. `msgq sessions` reports `address`, `alias`, `state`,
  and `expires_in`, but **not** the session id.

## Goals / Non-Goals

**Goals:**

- A human speaks and answers on the bus from the session's prompt, with the session's identity,
  without leaving pi and without spending a model turn.
- No new wire: the commands are a second caller of the planners that already exist.
- Keep the two audiences apart: what is shown to a human never enters the content the model reads.
- Every command is exercisable without herdr, without a peer, and without a live bus.

**Non-Goals:**

- No overlay, no keybinding, no live roster pane.
- No blocking command and no `--wait`.
- No roster cache and no address autocompletion.
- No new action for the model's tools, and no second way to publish.
- No `publish --meta`, and therefore no `meta.origin`.
- No bell and no desktop notification on arrival.
- No new capability: these requirements constrain the extension's own behavior.

## Decisions

### D1. The commands are a second face on the same planners

A command handler resolves its arguments into the same parameters the tool takes, calls the same
planner, and runs the same command line.

- Alternatives: publish through the engine handle the extension already holds for leases and
  acknowledgements.
- Rationale: the extension's *Scoped write exception* says it writes leases and acknowledgements and
  nothing else, and a message published through its own handle would break that invariant while
  also being a second implementation of publish. Going through the planner means the record a human
  sends is byte-for-byte the record the model sends, and the addressing logic that is already
  tested stays the only addressing logic.
- Consequence: each command costs one short-lived process. The model's tools already pay that, and
  it is a few milliseconds.

### D2. `me` is resolved by the extension, not by the bus

`me` resolves to the session's own address, in the extension, before the planner sees it.

- Alternatives: teach the CLI a `me` alias; look the name up in the roster.
- Rationale: the bus has no notion of self, and cannot acquire one — it sees senders and consumers,
  not sessions. The extension is the only component that knows which participant it is.
- Consequence: `me` is not a bus feature. It means nothing to `msgq` when typed at a shell, and the
  README says so.

### D3. A bare name gets its scheme, and a missing recipient is reported rather than refused

`reviewer` becomes `session/reviewer`. The command always reports the address it sent to, and
additionally reports when no participant currently holds it. The message is sent either way.

- Alternatives: resolve bare names against the live roster and refuse on a miss; refuse any send to
  an address with no subscriber; send silently.
- Rationale: the roster is a liveness snapshot, not a namespace, and refusing on a miss would make a
  momentary presence gap a hard failure. But sending silently into the void is worse than either,
  because a consumer that starts later begins at the head of its stream and will never see the
  record — so the report is the honest part, not the refusal.
- Consequence: a typo still publishes. **This is the most debatable call in this change**: the
  picker path (no address given) can only ever offer live peers, so the risky path is the one a
  human chooses deliberately, and the report tells them what actually happened.

### D4. One picker, two callers

`/peers` always opens it; `/peer` opens it when no address was given; both then compose with the
same editor.

- Alternatives: a picker in `/peers` and a prompt in `/peer`; completing addresses instead.
- Rationale: choosing a peer is one interaction, and the second caller is the same flow. Because
  the picker exists, `getArgumentCompletions` is not needed, which is what keeps the command from
  shelling out on every keystroke.
- Consequence: the address is typed or chosen, never completed.

### D5. Commands do not use the reporting address

`planPeerAction` falls back to the configured reporting address when a send names no recipient.
The command path passes none, so that fallback never applies.

- Alternatives: let `/peer "hi"` go to the reporting address, as a tool call does.
- Rationale: the reporting address is how a *companion* knows where to report without being told.
  A human typing `/peer` with no recipient is asking to choose, not asking to be silently routed to
  their parent.
- Consequence: a human must name an address or pick one, and the picker is the answer when they
  have not.

### D6. Composing is the editor, not a one-line prompt

`/peer reviewer "..."` sends what was typed on the command line; the interactive path exists for a
message worth writing, and is a multi-line editor that can be abandoned.

- Rationale: a message is text and is often longer than a command line. Splitting the two means the
  quick path stays quick and the deliberate path is deliberate.
- Consequence: abandoning the composition sends nothing, which is a scenario in the spec.

### D7. The reply affordance is rendering, and it names the sender

The delivered **content** is what the model reads. A line telling a human to type a command would be
an instruction to the wrong reader, so it goes in the renderer. The renderer names the sender the
answer would go to, as `/peer <sender> ...`.

- Alternatives: append the hint to the content; render `/reply`.
- Rationale: the content/model split is the same discipline the extension already applies by
  keeping `--json` and the human view apart. On the wording: `/reply` means "the message I most recently
  received", which is ambiguous on an older row of a coalesced batch, so the renderer states the
  address instead. It is always correct, for any row, and it teaches the syntax.

### D8. `/companions` goes through the `companion` command, never through herdr

The extension gains no herdr knowledge. A `list` verb is added to `companion/`, and the extension
runs it.

- Alternatives: the extension runs `herdr agent list` itself.
- Rationale: herdr is someone else's CLI with its own JSON and lifecycle, and the reason it lives in
  the shell command is so that exactly one file has to change when herdr does. The extension is
  portable; herdr is local. Doing the join there would also put half of it in each deliverable.
- Consequence: `/companions` and `companion list` fail together when herdr is absent, which is
  consistent with `spawn` and `close`.

### D9. `companion list` joins on the name

An entry is a named herdr agent that is also a bus participant, matched by the string that the herdr
agent, the pi session, and the address already share.

- Alternatives: join on the pi session id.
- Rationale: the id would be the stronger key, and `herdr agent list` does expose it — but
  `msgq sessions` does not, so matching ids would need a CLI change for a join the name already
  provides. The name is not a convenience here, it is the contract: one name, four places.
- Consequence: an unnamed herdr agent is invisible to the command, and a herdr agent renamed
  independently of its session stops joining. Both are honest: the set of companions is exactly the
  set `spawn` creates.

### D10. `/peers me` answers from memory

The identity report reads flags, startup state, and the presence handle. It does not read the bus.

- Rationale: it is precisely the command a human runs when something is wrong, and the most likely
  something is the bus. A report that needed the bus to explain a broken bus would be useless.
- Consequence: the report can say "the bus could not be opened" and still name the address, the
  policy, the topics, and the trusted senders.

### D11. Failure is reported, never thrown

A command that cannot do what it was asked reports why, at the level of the session's UI.

- Rationale: a command handler that throws in front of a human is a crash they cannot act on. The
  planner already distinguishes "cannot build this" from "here is a command", so the reason is
  already in hand and only has to be surfaced.

## Risks / Trade-offs

| Risk | Mitigation |
|---|---|
| A mistyped bare name publishes to an address nobody holds | The address and the missing recipient are both reported; the picker path can only offer live peers (D3) |
| A peer vanishes between the picker and the send | The send still reports the absent recipient; the record is durable, but a consumer starting later begins at its head, so the report is the only honest signal |
| Two vocabularies for state: the bus's `idle\|busy\|blocked` and herdr's `idle\|working\|blocked\|done` | Each command reports its own source and labels it, rather than inventing a translation that would be wrong at the edges |
| A herdr agent renamed without its session stops joining | The join is by name (D9); the mismatch shows its herdr detail as unknown rather than guessing |
| A command that needs a dialog on a session with none | Reported as needing one, and nothing is sent (Command resilience) |
| A human's message and their agent's are indistinguishable to a peer | Deliberate: one identity per session. `meta.origin` is the deferred answer, and it would be a hint rather than a guarantee |
| Every command spawns a process | The tools already do; the planners are pure and fast, and it keeps the extension out of the write path (D1) |

## Migration Plan

None required.

- Additive: no schema change, no protocol version bump, no new record type, and no CLI change.
- Nothing existing changes shape. The one existing behavior that changes is rendering, and only by
  adding an affordance; the content stays plain.
- Rollback is not using the commands: no storage is written and no bus state is altered by loading
  them.
- `/companions` requires `herdr`; the other three commands do not, so a session outside herdr keeps
  the surface it had.

## Open Questions

- Address autocompletion. It needs a roster that does not spawn a process per keystroke — either
  held in memory from the last roster read, or read once per command invocation. Deferred until
  typing addresses is actually the friction.
- Whether `/reply` should be able to answer a chosen message rather than the latest, now that a
  coalesced batch can put several candidates on screen.
- Whether the model's `companions` tool should gain `list`, now that the command can answer it. It
  is a one-line addition once `companion list` exists, and it is not needed for this change.
