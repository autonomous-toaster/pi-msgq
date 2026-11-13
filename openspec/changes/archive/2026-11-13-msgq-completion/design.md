## Context

See `proposal.md` for motivation and the delta spec for the contracts.

What already exists, and what it constrains:

- A peer that is asked a question is told, inside the delivered content, to answer with its reply
  tool. That hint is advisory: nothing enforces it, and the run simply ends if the agent ignores it.
- The reply path works when it is used: a reply is a turn-bearing record from the peer, so it wakes
  the parent through a rule that already exists.
- `ask` never had this problem, because the record it publishes requests acknowledgements
  (`meta: {ack: true, reply: true}`), and it ends with "finished without replying" on a terminal one.
  The model's `send` requests nothing.
- The extension's scoped-write exception: it writes presence leases and acknowledgements, and nothing
  else. It may run the CLI, which is how the model's tools publish anything at all.
- The delivery path deliberately holds no memory: a reference is resolved when it arrives, not
  remembered. The two exceptions are per-run (`inFlight`) and per-session (`lastInbound`).
- `lastInbound` is a single slot: the most recent record received that was not an acknowledgement.

## Goals / Non-Goals

**Goals:**

- A delegation ends in a signal, whether the peer answered, declined, or failed.
- The signal is the agent's own words, not the extension's.
- No new wire vocabulary, and no new actor on the bus.
- Knowing how often the rescue is needed, from the first day of use.

**Non-Goals:**

- No timer, no threshold, and no escalation tier.
- No receiver-side wake, and no decision in this change about whether the parent should ever be
  woken by the extension.
- No auto-reply, and no words put in the agent's mouth.
- No new record type, no CLI change, and no change to `ask`.

## Decisions

### D1. Repair the convention instead of adding a signal beside it

The fix is to make the reply that was already expected actually happen, at the moment it is about to
be dropped: when a run is stopping and a question is still unanswered, one more entry asks for it.

- Alternatives: a turn-bearing "no reply" record published by the peer's extension; a distinct
  lifecycle event on the bus; auto-replying with the agent's last text.
- Rationale: the convention is not broken, it is unenforced, and the agent already has the tool the
  entry asks it to use. A parallel signal would sit next to a working path that continues to be
  ignored, and it would have to be understood by every future reader as "the other way an exchange can
  end".
- Consequence: the parent is woken by a real answer through the path that already exists, so nobody
  has to decide whether the extension may spend a turn in someone else's session.

### D2. Declining is an answer, and the entry has to say so

The entry names both outs: answer it, or reply that nothing needed reporting.

- Alternatives: nudge only for an answer.
- Rationale: a peer may look at a question and legitimately conclude there is nothing to say. An
  entry that only accepts an answer teaches the opposite — that stopping quietly is a bug to be
  nagged out of the model, which is how a system trains an agent to pad its reports. The out already
  exists: a reply with any text works, and the asker's `ask` sees it as the answer.
- Consequence: silence stops being the only way to decline, and stops being indistinguishable from
  forgetting.

### D3. Bounded per question, never per run

The extension asks at most once for the question it is owed an answer about, and never again once
that question has been answered.

- Alternatives: ask on every settle while the question is open.
- Rationale: an entry that cannot tell "unanswered" from "answered" fires on every settle, which is a
  loop that bills. The bound is what makes the feature safe, and it is per question rather than per
  run because a question can outlive several runs.
- Consequence: one new piece of state, and it is deliberately tiny — the question that is currently
  owed an answer, and whether a reply has gone out for it. The reply paths are the extension's own
  (the tool and the command both build `--reply-to <offset>`), so it can see when one is sent.
  Bounded to a single slot: a session asked two questions at once is nudged about the later one,
  which is recorded as an open question rather than pretended away.

### D4. The extension still never speaks, and that is why this shape won

The nudge injects an entry; the agent answers; the reply is published by the CLI, like every other
message a session sends.

- Alternatives: the extension publishing the completion itself.
- Rationale: the scoped-write exception is the promise that this extension cannot start a
  conversation. Auto-replying, or publishing a "finished" record on the agent's behalf, would break
  it and would also mean the extension deciding what an agent meant to say. Here the extension only
  makes the agent speak — and the one thing it does write, the acknowledgement, stays exactly what it
  always was.
- Consequence: the answer is always the agent's own text, and the human reading the transcript sees
  who wrote it.

### D5. The completion report is legibility, not a wake

A message the model sends asks for a completion acknowledgement. It is delivered as a notification,
and it does not start a turn.

- Alternatives: leave `send` silent, since the nudge is supposed to make the reply happen anyway.
- Rationale: the nudge cannot reach a peer that crashed, was killed, or runs an extension older than
  this one, and those are exactly the cases where a sender would otherwise wait on nothing. But an
  acknowledgement has never woken a session and should not start now — that would be the
  receiver-side decision this change deliberately leaves alone.
- Correction, recorded because it is a mistake worth not repeating: this was first described as a
  "safety net" that closes the hole. It does not. A display-only line reaches an idle parent only when
  that parent wakes for some other reason, which is precisely when it did not need to know. Its value
  is that the transcript and the human stop being told a comfortable lie.

### D6. Measure it rather than tier it

The nudge reports when it fires. No ack-first-then-escalate threshold, and no timer.

- Alternatives: acknowledge first, and nudge only if the acknowledgement sits unanswered past a
  threshold.
- Rationale: the tiering is a sensible objection — do not spend model steps without evidence — but it
  buys that evidence with the outstanding-request table and the timer that this system has now twice
  declined, and it would fire *later* than the moment the answer is still in the agent's mind. A
  reported nudge gives the same answer about frequency on the first day, for nothing.
- Consequence: if the report shows the nudge firing often, the case for a cheaper tier is made by
  data rather than by caution.

### D7. What is deferred, and what would revive it

The receiver-side wake — letting the *parent's* extension decide whether to spend a turn — is the
better design in principle, and the council preferred it.

- Rationale: it needs the parent to remember what it sent and to know whether a reply arrived, which
  is the outstanding-request state D3 keeps to a single slot on the peer's side and which
  `msgq-notify` refused on the sender's side. It also needs a rule for when a turn is warranted, which
  is a timer by another name.
- Consequence: it is written down as the next step rather than dismissed, with the condition that
  revives it — a reported nudge frequency high enough to show that the cheap path is not carrying the
  load.

## Risks / Trade-offs

| Risk | Mitigation |
|---|---|
| The nudge loops and bills | At most once per question, and never once answered (D3) |
| It fires for a peer that deliberately declined | The entry names the out explicitly, and a "nothing to report" reply retires the question (D2) |
| A model learns that silence is a bug and pads every reply | The same out, plus the skill saying that declining is a legitimate ending |
| An extra model step per unanswered question | Only for a question that is genuinely owed an answer, bounded, and reported so the frequency is visible |
| One extra notification per delegation | It is a notification: no turn, and the alternative is silence |
| A peer too old, or too dead, to nudge | The completion report still reaches the sender, so the wait is informed even when the answer never comes |
| Only the most recent question can be owed an answer | Recorded as a limitation and an open question; a session asked two questions at once is nudged about the later |
| The state that detects "answered" could drift from the reply paths | The reply paths are the extension's own, and the slot is cleared where a reply is planned, not guessed |

## Migration Plan

None required.

- Additive: no wire change, no new record type, no CLI change, and no new dependency.
- What changes for a sender is that a message it sends now requests completion; the record differs
  only by a `meta.ack` it did not have, which the peer's extension already knows how to honour.
- Nothing changes for any other delivery path, and the nudge only ever adds one entry.
- Rollback is not asking for the acknowledgement; the nudge stands alone and is bounded.

## Open Questions

- Whether the human's `/peer` should also request completion. A person watching the transcript sees
  the answer arrive and does not need telling, but the same argument was made for the model and lost.
- Whether several outstanding questions need tracking. That turns one slot into a set, and the set
  needs bounding in turn.
- Whether "nothing to report" should be a shape on the wire rather than prose in a reply. Prose works
  today and needs no vocabulary; a shape would let a sender fold it mechanically.
- The receiver-side wake, deferred in D7, with the condition that revives it.
