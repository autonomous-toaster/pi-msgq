## Context

See `proposal.md` for motivation and the two delta specs for the contracts.

What already exists, and what it constrains:

- The extension delivers every record with `pi.sendMessage`: a **custom message**, wrapped in a
  provenance header the renderer and the model both see.
- `pi.sendUserMessage(content, { expandPromptTemplates })` also exists, and defaults that option to
  `false`. With it `true` it calls `prompt(text, { expandPromptTemplates: true, source:
  "extension" })`, which is the path a typed command takes.
- That path, in order: an extension command for a leading `/` runs immediately, even mid-run, and
  sends no prompt; then input handlers; then `/skill:name` becomes the skill body with its arguments;
  then a prompt template expands with its arguments; an unrecognized `/…` **passes through
  unchanged**; then, if streaming, the text is queued as a steer or follow-up, otherwise it is sent
  as a real user prompt and a turn runs.
- `prompt()` flushes pending custom messages **before** it validates the model and sends — so two
  calls land in the transcript in the order they were made.
- The record contract enumerates its types and requires consumers to tolerate unknown ones. The
  record version is `1`.
- Delivery is gated by the trusted sender set; a sender outside a configured set is display-only.
- Nothing about any of this is authenticated: `from` is unverified, as the specs already say of
  acknowledgements.

## Goals / Non-Goals

**Goals:**

- A peer can be handed a command and run it, instead of being asked to describe one it cannot run.
- The model can still tell that the instruction came from outside, and the human can still see who
  sent it.
- The bus's vocabulary says what a record *is*, in the place a reader already looks.
- Nothing already delivered changes shape.

**Non-Goals:**

- No second trust gate, no approval step, and no allowlist of commands.
- No reply mechanism for commands beyond what a template's own arguments already give.
- No change to how any other record type is delivered, and no version bump.
- No decision about the human's `/peer` marking a command, and no autodetection of a leading `/` in
  ordinary records.

## Decisions

### D1. A command is a record type, not a flag

`msgq publish <address> "/harsh-review …" --type command`.

- Alternatives: `meta.command: true` on a `prompt`; autodetecting a leading `/` in any record body.
- Rationale: the `type` field *is* the delivery axis — `prompt`, `steer` and `follow_up` already
  answer "how should this be delivered" — and a command is one more answer. It is then visible in
  the CLI's help, in the record, in the delivery rule, in the transcript badge, and in the contract,
  where a `meta` key would be invisible in all five. Autodetection is rejected on a concrete hazard:
  `/review`, `/plan`, `/refine` and `/scout` are template names that exist on this machine, so a
  message whose body happened to be one of them would silently become that command.
- Consequence, and the deciding argument: a peer too old to know the type treats it as display-only.
  A flag on a `prompt` would instead set a model a riddle, which is precisely the failure this
  change exists to remove. The version stays `1`: the envelope is unchanged, and tolerating unknown
  types is already required.

### D2. Dispatch is input delivery, and it is the same path a typed command takes

The body goes through `sendUserMessage(text, { expandPromptTemplates: true })` — no bespoke
expansion, no second implementation of command lookup.

- Alternatives: expanding prompt templates in the extension and sending the result as a prompt;
  dispatching only the templates and skills while refusing extension commands.
- Rationale: "as if typed" is the whole point, and it is also the smallest thing to maintain — the
  host already knows how to resolve a command, and it degrades in the host's own way: an unknown
  command passes through as text rather than erroring. Re-implementing expansion would fork the
  meaning of a command, and refusing the extension-command case would introduce a *new* silent
  failure, which is the class of bug this change is about.
- Consequence: a record can now run an extension command handler. See D5.

### D3. Provenance is a note sent immediately before the command

```text
   sendMessage(provenance note)      ->  "[msgq] from session/planner on session/reviewer"
   sendUserMessage("/harsh-review …") ->  the command itself, dispatched
```

- Alternatives: the renderer only (the model goes blind); accept the loss (the delivered message no
  longer identifies its sender).
- Rationale: the requirement that a delivered message name its sender and address exists *so the
  model can distinguish external messages from user input*, and a command delivered as a bare user
  message is exactly that case. The note is the provenance line the extension already produces, so
  nothing new is invented. This is a rule about any dispatched record rather than about commands, so
  the next input-shaped thing inherits it.
- Evidence, not assumption: `prompt()` calls `_flushPendingCustomMessages()` before it validates the
  model and sends the prompt, and the flush appends the pending entries to the transcript. The two
  calls therefore land in the order they were made.
- Consequence: if the dispatch fails — no model, no credentials, a compaction in progress — the note
  is left pending and lands later on its own. That is a stray provenance line without a command,
  which is reported like any other delivery failure and is harmless on its own.

### D4. A command is never coalesced

A command that arrives while the agent is running is queued as a steer or follow-up; it is never
gathered into a batch.

- Alternatives: treat it like any other turn-bearing record and buffer it.
- Rationale: a batch is delivered as one custom message, so a command inside one would stop being a
  command. This is the one place where the new type has to be excluded from an existing path rather
  than flowing through it, and it is worth stating because the buffer is the natural place for
  someone to generalize later.
- Consequence: commands bypass the coalescer, so several of them arriving during a run are queued
  and expanded individually. That is the correct reading of "run this now", and it is bounded by the
  trusted set.

### D5. The gate is the existing trusted set, and the escalation is written down rather than removed

- Rationale: an untrusted command is display-only, exactly as an untrusted turn-bearing record is
  today, so no new policy is needed and no existing behaviour moves.
- What must be said plainly, because it is new: dispatching goes **around the model**. A trusted
  sender can run an extension command handler, which is code, without anything deliberating in
  between; and `/skill:x` is dispatched even when that skill sets `disable-model-invocation`, a flag
  whose purpose is to keep a skill in human hands. The trusted set was already load-bearing; this
  makes it more so, and the honest framing is "safe by default, and the default is a delegation from
  an agent you started".
- Consequence: both edges belong in `design.md` and in the skill. Whether extension commands deserve
  their own gate is an open question, answerable only after the feature has been used.

### D6. The answer rides in the command's arguments, and `ask` refuses

A prompt template takes arguments, so the sender writes the task and the request for an answer into
the command line: `/harsh-review review the parser and reply to session/planner with your findings`.

- Alternatives: wrap the command in a reply instruction; give commands their own reply mechanism.
- Rationale: neither exists and neither can — the reply instruction is injected into the content,
  and a command must be the whole text with nothing prefixed or appended. But the template's own
  argument slot is *already* a place for the sender's words, and it sits inside the instruction the
  model reads. So the answer comes back by the ordinary reply path, with no new mechanism at all.
- Consequence: asking a command is refused by the tool rather than silently wrapped, and the refusal
  names the fix. This is knowledge for the skill, not machinery.

### D7. The human's command surface is left alone

`/peer reviewer "/harsh-review …"` cannot yet mean a command. A flag, a prefix, or reading a leading
`/` after the quotes are all available, and a person rather than a model is the one who will notice
the difference, so it is not decided here.

- Consequence: the capability is reachable from the shell with `--type command`, and from an agent,
  which is where the failure was observed.

## Risks / Trade-offs

| Risk | Mitigation |
|---|---|
| A bus record can run extension command code with nothing deliberating in between | The trusted set gates it, parent-only by default; the escalation is stated in the specs' rationale, here, and in the skill, rather than discovered |
| `/skill:x` bypasses `disable-model-invocation`, which exists to keep a skill in human hands | Named as an accepted edge; an explicit gate for extension and skill commands is an open question |
| A failed dispatch leaves a provenance note with no command behind it | The note is harmless alone and the failure is reported like any other delivery failure |
| The coalescing exclusion regresses when the buffer is next touched | The rule is a scenario in the spec, not a comment in the code |
| A sender addresses a command to a session whose commands it does not know | An unknown command passes through as text, so the worst case is an odd prompt, not an error |
| Two entries per command is more transcript noise than one | It is the only place the sender can appear, and it applies only to command records |
| The type is invisible to an older peer, so a command is merely shown | That is the intended degradation, and the argument for choosing a type over a flag |

## Migration Plan

None required.

- Additive: a new type value, an unchanged envelope, and an unchanged version. Consumers are
  already required to tolerate types they do not know.
- Every other record type is delivered exactly as before; the new path is reached only by
  `type: "command"`.
- Rollback is not publishing the type: nothing else reads it, and an older extension shows it like
  any unknown type.

## Open Questions

- Whether extension commands deserve a gate of their own, separate from the trusted set. This is the
  one new thing that runs code without a model in the loop, and it will be easier to judge after some
  use than before.
- Whether a dispatched `/skill:x` should honour `disable-model-invocation`. The flag says "human
  hands only", and a dispatched command is a human-ish hand holding an agent's pen; reasonable people
  would read that either way.
- How a person marks a command from `/peer`, if at all.
- Whether the preceding-note shape should become the general way a turn-bearing record keeps its
  provenance, rather than something commands need because they cannot carry one.
