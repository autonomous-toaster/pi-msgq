## Context

See `proposal.md` for motivation and the two delta specs for the contracts.

What already exists, and what it constrains:

- The model has `peers` (list, send, ask, reply) and `companions` (spawn, close). `peers list` is a
  plain passthrough of `msgq sessions`: the extension formats nothing.
- The human has `/peers me`, which reports address, bus location, subscriptions, policy, presence and
  the trusted senders. The model has no equivalent.
- `companion spawn` sets `--msgq-report-to "$parent" --msgq-trust "$parent"` — one address, hardcoded.
  It exports `MSGQ_DEPTH` into the tab, which the extension **never reads**, and does not export
  `COMPANION_MAX_DEPTH` at all.
- Delivery policy: a trusted set is optional, an empty one means no allowlist, and with a non-empty
  one a record from any other sender is delivered display-only. Trust is compared literally against
  the record's `from`.
- The address grammar validates only the scheme, so `--from` may be any address — which is why a
  sender is unverified, and why the trusted set is a scope rather than an authentication boundary.
- The extension ships one skill, declared by the `pi` manifest in `extension/package.json`.

## Goals / Non-Goals

**Goals:**

- An agent can answer "who am I, and what am I allowed to do" without being refused first.
- An agent has the knowledge to delegate well: what a task message must contain, when to block and
  when to be woken, and which of its instincts fail silently.
- The topology is the caller's decision, defaulting to the star that exists today.

**Non-Goals:**

- No mesh protocol, no relay, no sibling addressing, no group messaging.
- No trust set that is updated at runtime; it is fixed at spawn.
- No handover, no spawn-with-task, no reaping, and no record of which children a session started.
- No change to the wire, the four human commands, or the delivery rules themselves.

## Decisions

### D1. The model's identity is the header of the roster

`peers list` reports who the session is, then the reachable peers.

- Alternatives: a new `peers` action (`identity`); a notice pushed into the transcript at session
  start.
- Rationale: "who is out there" and "which of them is me" are the same question, and a session about
  to delegate needs both. A separate action is the kind that never gets called, because a model
  rarely asks who it is — it assumes it knows. A start-of-session notice would put the facts in every
  session's first turn, whether or not anything bus-related happens.
- Consequence: `list` stops being a passthrough. The roster text itself is unchanged and the identity
  is prefixed, so the only risk is in the formatting, not in what the bus returns.

### D2. One report, two audiences

The identity report is composed by one pure formatter, used by both `/peers me` and `peers list`.

- Alternatives: a model-shaped report and a human-shaped one, kept separate.
- Rationale: they answer the same question, and two implementations of "what am I" drift. This is the
  same shape the command change already established: one pure function, several callers.
- Consequence: the human's report gains the two facts the model needs — the recipient it defaults to,
  and its delegation depth and cap when it has one — so the requirement that enumerates those facts
  has to name them.

### D3. Depth is stated, not discovered

The extension reads `MSGQ_DEPTH`, and `companion spawn` exports `COMPANION_MAX_DEPTH` alongside it.

- Alternatives: leave it, and let a delegate learn by being refused.
- Rationale: the refusal happens at the grandchild's attempt, which is far from the decision that
  made it impossible. A delegate that knows it is at the cap plans differently — it does the work or
  asks its parent for a wider spawn — instead of building a plan that cannot run.
- Consequence: one more environment variable in the tab, and the extension reads a variable the
  companion command owns. That coupling already exists for `MSGQ_PARENT`.

### D4. The trusted set is an option, defaulting to the parent

`companion spawn --trust <addresses>`, with the parent alone as the default.

- Alternatives: a group address as the parent (reachable today: `--parent group/family` makes the
  companion trust that group, and `--topics` makes its members follow it); a runtime update; a
  wildcard grammar.
- Rationale: the group trick gets *half* the way and is worth knowing before designing anything —
  the family hears the group, but a member answers as **itself** (`--from session/<name>`), and its
  siblings trust only the group, so they ignore each other. Closing that gap by letting peers speak
  as the group would mean faking `from`, which is the one field a peer must not fake. A list the
  caller names is the smallest thing that works, and it needs no new concept.
- Consequence: the sentence the design has been asserting — "a companion answers its parent and
  nobody else" — becomes a *default* rather than a property. Nothing about the wire changes.

### D5. The allowlist is a scope, not a security boundary

This is a correction worth writing down, because it is the reason D4 is cheap rather than dangerous.

- Rationale: `from` is unverified — the address grammar accepts any name, and nothing signs a
  record. Anything that can write the bus file can already claim to be a parent. So the trusted set
  never authenticated anyone; it decides which senders a shell-capable agent will *routinely* obey.
  The real boundary is who can write the file.
- Consequence: widening it changes accidental blast radius, not an authenticated permission. The
  honest framing for the default is "safe by default", and for the docs "scope", not "security". It
  also means the widest useful set is still worth keeping small: `session/*` would make an agent obey
  every session on the machine.

### D6. Two skills, split by direction

`msgq-notify` (a process outside reaches a session) and `msgq-delegate` (this session reaches
others).

- Alternatives: one skill covering the bus.
- Rationale: the audiences differ in kind — one is writing a shell command, the other is deciding to
  delegate — and a single description broad enough to cover both is exactly what makes skill routing
  fail. The cost is that the wire rules appear in both, which is cheaper than the alternative of
  pointing one skill at the other's file and hoping the model follows.
- Consequence: two descriptions must be checked against each other, so that "session" and "message"
  in one do not make the other unreachable. And each skill must be short enough that loading the
  wrong one is not expensive.

### D7. The skill teaches the traps, because the traps are the content

The delegation skill is mostly about what fails silently:

- the spawn result's **granted** name, which may be suffixed, versus the one that was asked for;
- `ask` blocks and `send` does not, and a woken reply is legible thanks to the reference annotation;
- a companion's answer is unverified text from another agent;
- following is not trusting;
- nothing reaps a finished companion, and nothing records which children are yours;
- a companion is a full agent: its own context, its own cost, and breadth is uncapped.

- Rationale: none of this is in a tool description, and all of it is knowable without any new
  mechanism. It is the clearest case in this repository for a skill rather than code.

## Risks / Trade-offs

| Risk | Mitigation |
|---|---|
| Widening the trusted set widens accidental blast radius | Parent-only by default; the set is reported back to the session itself, so it is inspectable rather than implicit |
| A delegate obeys more senders than the operator expected | The set is named at spawn, passed explicitly, and visible in the identity report |
| `peers list` stops being a passthrough, so a formatting bug hides the roster | The roster text is unchanged and the identity is a prefix; an end-to-end scenario asserts both parts |
| A second skill costs prompt space in every session that loads the extension | One name and one description each |
| Renaming a spec requirement (`Shipped skill` → `Shipped skills`) | Expressed as RENAMED plus MODIFIED in the delta, so the merge renames rather than duplicating |
| The delegation skill is not loaded when it is needed | Its description names the *task* — delegating work, a second opinion, running things in parallel — rather than the mechanism |
| Breadth is uncapped: an agent can spawn many companions | The skill says what a companion costs and to close what it started; a cap on breadth is a separate decision |
| An agent that knows it may delegate over-delegates | Depth caps the chain, the skill states the cost, and the parent is the one paying |

## Migration Plan

None required.

- Additive for the command and the wire: the default trusted set is exactly what the script already
  did, and the new option only widens it deliberately.
- `peers list` output gains a header, and `/peers me` gains two lines. Both are additive; no existing
  field changes meaning or disappears.
- A skill added to `extension/skills/` is declared by the manifest that already exists; the manifest
  is unchanged.
- Rollback is not using `--trust`: the default behaviour is preserved.

## Open Questions

- Whether a header on `peers list` is actually read, or whether the model needs an explicit action.
  That is answerable from real use, and it is the one decision here I would revisit first.
- Whether a session should be able to ask which companions **it** started. Nothing records parentage
  today, and a stateless answer would need the presence lease to carry it, which is a grammar change.
- Whether a trust set that can be updated at runtime is ever wanted. This change deliberately refuses
  it, because a delegate's siblings may not exist yet when it is spawned.
- Whether `/peers me` should keep growing, or be split once it reports more than a screen.
