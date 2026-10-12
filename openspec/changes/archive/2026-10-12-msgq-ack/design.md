## Context

See `proposal.md` for motivation and the two delta specs for the behavior contract.

Four facts shape the approach:

- `reply_to` has been part of the record contract since the beginning and has never been written.
  Stream offsets are globally unique across topics (one `AUTOINCREMENT`), so a single offset
  identifies a record everywhere.
- The extension already observes the run boundaries an acknowledgement needs: `agent_start`,
  `agent_settled`, and `agent_before_settle` (added for presence).
- The extension has a scoped write exception for leases, and an address model where publishing to
  a sender's address lands on the sender's own stream, which the sender already follows.
- The binding's subscription saves the cursor **before** it yields an event, while the coalescer
  holds turn-bearing records in memory — so a stop mid-run consumes records it never acted on.

## Goals / Non-Goals

**Goals:**

- A sender can learn that a message was received, started, and completed.
- Correlation uses the offset the bus already assigns; no new identifier scheme.
- Acks are ordinary records, so every existing reader tolerates and ignores them.
- Durability stops lying: nothing is dropped silently between delivery and action.

**Non-Goals:**

- Result payloads, resumption/wake, progress, and cancellation.
- Any change to acknowledgement trust; acks are as unverified as `from`.
- Exactly-once acknowledgement. The contract is at-least-once with a foldable identity.

## Decisions

### D1. Acknowledgements are ordinary records

Emit `type: "event"` with `event.kind: "ack"`, rather than a new record type or a side channel.

- Alternatives: a new `ack` record type; a dedicated acknowledgement stream; a `_honker_results`
  entry per offset.
- Rationale: `event` is already display-only for every existing consumer, so acknowledgements
  cannot disturb them, and the record contract is untouched (no version bump). A side channel
  would need its own reader, retention, and addressing.

### D2. Three phases, with the delivery mode on `received`

```
  publish        responsibility        run starts          run settles
     |                 |                   |                    |
     v                 v                   v                    v
  [message] ------> [received] -------> [started] --------> [completed]
                     delivery:           (a run began)        outcome:
                     display | turn |                         ok | error | aborted
                     queued
                     |
                     +-- delivery: display  = TERMINAL (no turn will run)
```

- Alternatives: a strict two-phase lifecycle (started, completed); adding a `skipped` phase.
- Rationale: `received` is the cheapest and most diagnostic phase — it separates "queued on an
  offline session" from "delivered but stuck". Carrying the delivery mode on it makes a
  display-policy delivery explicitly terminal, which avoids the false-stuck signal a two-phase
  ladder would create. `completed` reports an outcome rather than adding phases, keeping the
  vocabulary small.

### D3. Correlation by reply reference; duplicates folded by tuple

`reply_to` = the original record's offset. The acknowledgement identity is
`(reply_to, acknowledging sender, phase)`.

- Alternatives: a generated message id; a monotonic ack sequence per session.
- Rationale: the offset is already unique and already in the contract. A reader folds duplicates by
  that tuple without extra bookkeeping, which matters because the binding can replay one record
  after an interruption.

### D4. The caller opts in with `meta.ack`

- Alternatives: a first-class optional `ack` field; acknowledging every delivered record.
- Rationale: per-message opt-in keeps the bus quiet when nobody asked. `meta` avoids extending the
  contract while the shape is still moving (results and wake are deferred and will add fields).
- Recorded cost: this puts a protocol instruction in a field documented as "never read by the bus".
  Accepted deliberately; promoting it to a first-class field later is additive.

### D5. Notification only

An acknowledgement never starts a turn.

- Alternatives: let a terminal ack wake the sender.
- Rationale: resumption is only useful with result payloads, which are deferred, and it introduces
  the loop risk that the security thread already flagged. Shipping notification first keeps this
  change small and safe, and the addressing is designed so `wake` can be added later.

### D6. Responsibility-based cursor saving

Persist the consumer offset when the extension takes responsibility for a record, not when the
binding yields it.

```
  today:  binding saves at delivery --> coalescer holds in memory --> stop = LOST
  now:    save when responsible
            display  -> saved after it is shown
            turn     -> saved after it starts a turn
            queued   -> saved when the batch is injected at settle
```

- Alternatives: keep saving at delivery and persist the coalescer across restarts; leave it and
  let acknowledgements expose the loss as a timeout.
- Rationale: leaving it makes `received` a false claim. Persisting the buffer would preserve the
  old guarantee but adds machinery and silently fails for `--no-session`. Saving at responsibility
  turns a silent loss into an at-least-once redelivery, which a foldable ack identity makes cheap.
- **This is a behavioral change**: the guarantee moves from "at most one record replays" to
  "anything not yet acted on may replay". It is why `Cursor durability` is modified rather than
  extended.

### D7. Awaiting records the head before publishing

`--await` resolves the reply address, records that stream's current end, publishes, then reads
from the recorded position filtering by the published offset.

- Alternatives: publish first and then start reading.
- Rationale: an acknowledgement can land between the publish and the read, and would then be
  missed forever. Snapshotting first makes the wait race-free, and it reuses the CLI's existing
  offset-based read.

### D8. Timeout is a distinct exit status

Waiting ends on a terminal acknowledgement, or on a timeout. A timeout exits `3`.

- Alternatives: reuse `1`; treat a timeout as success with no output.
- Rationale: a script must distinguish "the work finished" from "it never answered"; neither is an
  error in the runtime sense, so neither `0` nor `1` is honest. `--timeout 0` waits without limit,
  and the default window is finite so an offline session cannot hang a script forever.

### D9. Acks are session-level, and the spec says so

An acknowledgement reports that the session took responsibility, began a run, and settled — not
that it reasoned about one specific record.

```
   msgq A --> delivered (turn)  --.
   msgq B --> delivered (note)     \   ONE run, context holds A, B, C
   user typed C -------------------/
   acks:  A received/started/completed   B received only   C none
```

- Rationale: per-message causation is not observable from the extension's vantage point. Stating
  this keeps the contract honest and stops a consumer from treating `completed` as proof that a
  particular instruction was followed.

### D10. Resilience and the anti-loop rule

A failed acknowledgement publish is non-fatal and reported; an undecodable record is never
acknowledged (its envelope cannot be trusted); an acknowledgement is never acknowledged.

- Rationale: acknowledgement emission must not be able to break consumption, and the
  no-ack-of-ack rule removes an obvious feedback path before resumption is ever considered.

## Risks / Trade-offs

| Risk | Mitigation |
|---|---|
| Duplicate acknowledgements after an interruption | Foldable identity `(reply_to, sender, phase)`; consumers fold to the latest |
| A `started` acknowledgement can be missing if the process dies inside a run | Accepted; the sender's timeout is the signal, and the work is redelivered (D6) |
| Acknowledgements are unverified and forgeable by any local process | Documented as advisory, exactly like `from`; orchestration on acks must add trust (deferred with `wake`) |
| Acknowledgement volume doubles bus traffic | Opt-in per message (D4) |
| `meta.ack` is a protocol field in a free-form bag | Recorded as a deliberate trade-off (D4); promotion is additive |
| The durability change loosens a shipped guarantee | Called out as a modification, with the trade-off stated (D6) |
| A display-only `prompt` looks like a failure to a naive sender | `received` carries `delivery: display` and is terminal (D2) |

## Migration Plan

None required.

- Acknowledgements are ordinary `event` records, so older readers tolerate and ignore them.
- No schema change and no protocol version bump; `meta.ack` is ignored by anything that does not
  read it.
- Rollback is reverting the extension and CLI; acknowledgement records already written remain
  ordinary display-only events.

## Open Questions

- Result payloads: bounded inline text versus a pointer to the child's record or session entry.
- Resumption (`wake`): which senders may wake a session, and what loop guard bounds it.
- Progress acknowledgements for long tasks.
- Promoting `meta.ack` to a first-class optional record field if the shape settles.
