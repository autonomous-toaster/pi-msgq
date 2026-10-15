## Why

A delegation has one ending that works and one that is silent. When the peer replies, the reply wakes
the parent and everything is fine. When the peer's agent finishes its run without replying — it
forgot, or decided the work did not warrant an answer — the parent is never told anything, and
silence is indistinguishable from still working.

Two changes ago that silence was a decision, written into `msgq-notify` as *"nothing will ever tell
you that a peer did not answer"*. It is the wrong decision for the case the same skill recommends —
send the task, carry on, be woken by the answer — because the sender that chose not to wait is the
one left uninformed. The blocking `ask` path has never had this problem: it requests lifecycle
acknowledgements, so it ends with "finished without replying".

The convention is also not broken, it is merely **advisory**. A peer asked a question is told, inside
the delivered content, to answer with its reply tool. Nothing enforces it, and the moment it slips is
the moment the agent decides it is done.

## What Changes

- **Ask for the answer at the moment it is about to be forgotten.** When a run is about to stop and a
  question the session was asked is still unanswered, the extension delivers one more entry telling
  the agent to answer it — **or to reply that nothing was needed**. Once per question, never twice,
  and it reports when it fires.
- **Report completion to whoever sent, whether or not they waited.** A message the model sends asks
  to be told when the run that included it completes. The report is delivered as a notification, so
  it informs the transcript and the human without spending a turn.
- **Teach the ending.** The skill that says "send the task and be woken by the answer" gains its other
  half: a delegation ends in a signal, and "nothing to report" is a signal.

Explicitly not in this change:

- No new signal on the wire, no new record type, and no CLI change: an acknowledgement already
  carries a completion, and a reply already wakes the parent.
- The extension still never speaks. The nudge does not answer on the agent's behalf — it makes the
  agent answer, through the reply tool it already has. Auto-replying with the agent's last text was
  rejected: it puts words in the agent's mouth, and that text is often mid-flow.
- No timer, no threshold, and no escalation tier. Ack-first-then-nudge-on-timeout was considered and
  deferred: it needs the outstanding-request state this system has twice refused.
- No receiver-side wake. Letting the parent's extension decide whether to spend a turn is better in
  principle and needs that same state, so it is deferred with the condition that would revive it.

## Capabilities

### New Capabilities

None. All of it is behavior of the extension that already delivers and acknowledges.

### Modified Capabilities

- `msgq-extension`: add the nudge that closes the reply convention, and request a completion report
  for what the model sends.

## Impact

- No new dependency, no new process, no CLI change, and no wire change. `publish --ack` and the
  acknowledgement lifecycle already exist; only who asks for one changes.
- The extension keeps its scoped-write promise exactly: the nudge injects an entry, the *agent*
  replies, and the reply is published by the CLI like every other message the session sends.
- One new piece of light state, bounded to a single slot: whether the question that is currently
  owed an answer has been answered. Bounded because the delivery path has otherwise avoided memory.
- A nudge that could not tell "unanswered" from "answered" would loop and bill. Bounding it per
  question, rather than per run, is what makes it safe.
- Every delegation now costs one extra notification record. Cheap, and the alternative is silence.
