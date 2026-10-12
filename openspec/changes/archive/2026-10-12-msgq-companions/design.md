## Context

See `proposal.md` for motivation and the three delta specs for the contracts.

What already exists, and what it constrains:

- The bus is complete: durable topics, presence as renewed engine leases, acknowledgements with
  phases, `msgq await`, and an extension that consumes, delivers under a policy, and acknowledges.
- The extension has **no tools at all**, so no agent can currently use any of it.
- `herdr` 0.8.2 provides tab creation with `--cwd` and `--env`, `agent start --kind pi` that waits
  for interactivity, per-agent `cwd` and `foreground_cwd`, `agent_status`
  (`idle|working|blocked|done`), and `HERDR_*` variables in every managed pane.
- `pi` provides `--name` (which becomes the session name, and therefore the extension's alias and
  address), `--approve` / `--no-approve` for project trust, and `-e` for a development extension.
- A new consumer starts at the **head** of its stream, so a message published before its
  subscription initialises its cursor is treated as history and skipped.

## Goals / Non-Goals

**Goals:**

- An agent can delegate to a real peer and receive the answer, with two tools and one command.
- No new storage, no daemon, no new record type, and no protocol change.
- One implementation of the wire, one of the herdr glue, and the extension as a thin adapter.
- A shell user can do everything an agent can.

**Non-Goals:**

- Handover and context transfer; orphan reaping; attachments; cross-machine peers.
- Result payloads in acknowledgements and resumption by waking.
- In-process subagents: a companion is a real `pi` process in its own tab.

## Decisions

### D1. Three layers, split by what each depends on

The wire lives in the Rust CLI, the herdr glue in a shell command, and the pi-specific parts in the
extension.

- Alternatives: everything in the extension; everything in Rust; one artifact.
- Rationale: the protocol must be identical wherever it is implemented, so it belongs in one place
  with tests. herdr is someone else's CLI with its own JSON and lifecycle, and shell is the honest
  place for glue: readable, editable, reusable by a human, and loud when herdr changes. Only the
  extension can see the session.
- Consequence: a person can run the whole flow from a shell, and the extension carries almost no
  logic.

### D2. Two tools, named for intent

`peers` (list, send, ask, reply) and `companions` (spawn, close).

- Alternatives: one tool with seven actions; `agents` for the wire.
- Rationale: one is "talk to someone", the other is "start or stop a process". `agents` would
  collide conceptually with in-process *subagents*, which these deliberately are not. If the model
  ever confuses the two tools, merging them is a small change, because the implementation split is
  unaffected.

### D3. Spawning is process-only

`spawn` starts a peer; the task is simply the first message.

- Alternatives: a `--task` argument sending the task after startup; passing the task as an initial
  prompt.
- Rationale: one code path for the first message and every later one, and the script stays free of
  message logic. Passing the task as an initial prompt would make the child one-shot instead of a
  conversational peer.

### D4. Readiness is presence, and that is not decoration

`spawn` blocks until the companion appears on the bus.

```
  pi boots -> extension subscribes (cursor := head) -> presence lease
                                                            ^
                        safe to send only after this point --+
```

- Alternatives: rely on `herdr agent start` returning; poll `herdr agent list` for `idle`; sleep.
- Rationale: a new consumer starts at the head of its stream, so a message published a moment too
  early is skipped as history and lost. Presence is registered only after the subscription exists,
  so it is a deterministic proof of readiness rather than a guess.
- Consequence: a companion must have presence enabled. A spawn against a session with presence off
  can never pass the gate, which is reported rather than waited out.

### D5. One name, four places

The herdr agent name, the pi session name, and the address local part are the same string, and the
spawn result reports it. herdr's grammar (`[a-z][a-z0-9_-]{0,31}`) is enforced so one name suffices.

- Rationale: the parent knows where to reach a companion because it chose the name; no registry
  lookup is needed for the spawn flow, and the address is predictable from the tab.

### D6. Deduplicated names

A taken name yields a suffixed alternative; the address follows the granted name, and attempts are
bounded.

- Rationale: two companions must never share a stream, and returning the granted name means the
  caller never has to guess.

### D7. `ask` is a CLI verb that blocks

`msgq ask` publishes and waits, ending on a reply, on a terminal acknowledgement, or on its window,
and none of those is an error.

- Alternatives: an in-process promise registry in the extension; fire-and-forget plus a wake
  feature; result payloads in acknowledgements.
- Rationale: making the wait a CLI verb means one implementation, testable from a shell, and the
  extension needs no blocking machinery at all. Ending on a terminal acknowledgement is what stops
  an ask hanging when a peer finishes without replying, so acknowledgements do double duty.
- Consequence: the parent's turn is occupied while it waits — honest, since it is genuinely blocked
  on the answer — and the human can always abort.

### D8. Replies need no prior knowledge

`reply` addresses the inbound record's sender and sets `reply_to` to its offset, so a companion can
always answer whoever asked. The parent address is only needed for unsolicited sends.

- Rationale: it removes a whole class of "where do I report to?" configuration from the child.

### D9. The parent address travels on two channels

Passed both as an environment variable and as a pi argument.

- Rationale: the argument is explicit and visible in the process list and in the child's own help;
  the environment is inherited by anything the companion starts, including its own companions.

### D10. Working directory and project trust are explicit

`--cwd` defaults to the caller's directory; project trust is granted or withheld explicitly, and a
blocked startup is reported instead of looking like a hang.

- Rationale: the working directory decides which project resources the companion discovers, so it
  is often the reason to spawn at all. A herdr tab is interactive, so an untrusted directory will
  prompt and stall, and the failure mode must be legible.

### D11. Depth is capped

A companion is told its depth and refuses to spawn past a cap unless overridden.

- Rationale: companions can spawn companions, which is a fork bomb with a model in the loop. One
  level covers the intended delegation.

### D12. Discovery is split on purpose

`peers` reports who is reachable, from the bus, and stays portable and lean. `companions` reports
where they are and what they are doing, from herdr.

- Alternatives: encode a working directory into the presence lease; a TTL'd metadata blob.
- Rationale: a lease carries only a name and an owner, and cramming a path into a lease name is the
  same cleverness already rejected for state. herdr already knows the working directory and the
  status, exactly and locally.

### D13. Installed by default, injectable for development

`MSGQ_BIN`, `MSGQ_COMPANION`, and `MSGQ_SPAWN_EXTRA` override their names on `PATH`, and `--dry-run`
prints the commands without running them.

- Rationale: production spawns no `-e`, because the extensions are installed; a working tree
  injects them through one variable. `--dry-run` is the test seam that makes shell glue testable
  without a live herdr, and doubles as the debugging view.

### D14. Trust gates honoring

Under `honor`, only senders in a configured trusted set can start a turn.

- Rationale: a companion runs with `honor`, which makes it a shell-capable agent that a message can
  command. Spawn allowlists the parent, so a companion answers its parent and nobody else.

## Risks / Trade-offs

| Risk | Mitigation |
|---|---|
| A cross-directory spawn stalls on the project-trust prompt | `--trust` / `--no-trust` map to pi's approve flags; a blocked startup is reported, not waited out |
| Presence disabled means the readiness gate can never pass | Report it explicitly rather than hanging; document that companions require presence |
| Two companions share a name and a stream | Deduplicated names, bounded attempts (D6) |
| A spawn chain becomes a fork bomb | Depth cap (D11) |
| Each companion is a full agent with its own context and cost | Depth cap, and `close`; the human decides how many |
| A blocked `ask` occupies the parent's turn | Honest blocking, abortable by the human, and bounded by the wait window (D7) |
| herdr's CLI or JSON changes under us | The script is the only place that knows herdr, and `--dry-run` exposes what it would run |
| `herdr` or `jq` missing | Fail with a clear message naming the missing requirement |

## Migration Plan

None required.

- The command and the tools are additive; nothing existing changes shape.
- Rollback is not using them: no storage is written, no schema changes, and any companions are
  ordinary pi processes in ordinary tabs.
- `msgq ask` and `--reply-to` are additional verbs on an unchanged record contract.

## Open Questions

- Handover and context transfer, deferred: it needs a model call and a summary format.
- Orphan reaping: a companion that never finishes is left for the operator, by decision.
- Cross-machine companions, and whether herdr's remote sessions change the address model.
- Whether `peers` should enrich its roster with herdr status when it happens to be available.
