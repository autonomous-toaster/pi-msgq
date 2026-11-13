# Design

## The fault, precisely

A session has one execution slot. `ask` held it across a wait for work that could only be done by a
slot. Two shapes of that:

| shape | who waits | ends when |
|---|---|---|
| `A ask B` | A's whole turn | B answers |
| `A ask B`, `B ask A` | both turns | one of the two timeouts expires |

The second is not a slow version of the first, it is a different outcome: no progress can be made,
because the record that would let each side answer is held in a buffer that is only drained when the
blocked turn ends. Measured on the bus: unbounded waits never return; bounded waits return only at the
bound and then re-block.

## Why the wait, not the cycle, is what has to change

Three fixes were on the table.

**1. Detect the cycle and break it (kept, but insufficient).** The extension sees every inbound record
while the tool call is blocked, so it knows when the peer it is waiting on asks it something. That
handles A ⇄ B in milliseconds. It does not handle A → B → C → A: when C asks A, A is waiting on B, not
on C. A trace identifier carried in `meta` and inherited by every nested ask would catch arbitrary
chains, but it needs a new CLI flag, a new state to inherit, and it still only *detects*; every
undetected shape is still a deadlock. Kept as the cheap mitigation, not as the fix.

**2. Bound the wait (kept).** Turns every deadlock into latency. Cheap, and worth having whatever
else changes, but a bounded deadlock is still a stall the user has to sit through, and the cycle of
three still costs three bounds.

**3. Do not wait (chosen).** If no session holds its execution slot across a wait, no wait-for graph
exists and no cycle can form. The answer arrives the way every other message arrives: a record on the
session's own stream, delivered as a turn. This is what `send` already does, and the machinery that
makes it usable is already there — the reply carries `reply_to`, delivery resolves the reference, and
a session that is busy gets the answer coalesced into its next run.

The cost is honest: an answer can no longer be used inside the turn that asked. That turn has to end.
For an agent that was going to be woken by the answer anyway this is no loss; for a quick question to
an idle peer it is one extra turn, which is why the bounded opt-in wait is kept rather than removed.

## The surface

```
peers({action:"ask", to, message})                     publish and return
peers({action:"ask", to, message, wait:true})          wait, at most 120s
peers({action:"ask", to, message, wait:true, timeoutSeconds:30})
```

- The clamp is the extension's, not the CLI's: a script may legitimately `msgq ask --timeout 0`, and
  that stays. What the *model* can ask for is bounded, because a model choosing "no limit" is how the
  permanent deadlock was reachable at all.
- `wait:true` on the peer that is asking you is not refused; it is abandoned when their question
  arrives. Refusing would need the cycle knowledge this design deliberately does without.
- A question to this session's own address is refused: the delivery path is the only way a question
  reaches a session, and it never starts a turn on a record the session sent itself.

## Not delivering an answer twice

A blocking `ask` returns the answer in the tool result *and* leaves the record on the stream, where
the ordinary path delivers it again. The extension is the one component that knows both facts, so it
is the one that resolves them:

- It learns whether the wait ended with an answer or with an outcome. `msgq ask --json` prints the
  record when there is one and `{"outcome":…}` when there is not, so this is a shape check rather
  than a match on prose.
- While a turn is in progress it remembers the addresses whose answer it already handed to the model.
  A record from one of them that refers to a record this session authored is that answer, and is
  consumed instead of injected. Cleared at the start and the end of a turn, so it can never suppress
  a later, genuine message.
- A wait that ended without an answer (timeout, or the peer finished without replying) suppresses
  nothing: the answer, if it comes later, is delivered normally. That is the whole point of the
  non-blocking default.

## Why the CLI grows `ask --no-wait` rather than the extension learning to build a question

A question is not quite a `publish`: it carries `meta.ack` and `meta.reply`. Two places building it
means two definitions of what a question is, and the two would drift. `--no-wait` keeps one
definition and one record shape, and it is additive: a client that ignores it reads the same records.

## Rejected

- **Queue the peer's question as a `steer` and hope the model answers mid-run.** It cannot: the tool
  call is still returning. Injection without a free slot changes nothing.
- **Make the CLI deliver the answer itself and skip the bus.** Then the answer is not a record, is not
  visible to the human, and cannot be inspected with `msgq read`.
- **Drop the blocking path entirely.** It is the ergonomic one-turn exchange for a peer that answers
  in seconds, and the CLI needs it for scripts. Removing it would be a bigger change than the bug.
- **Auto-reply on the peer's behalf so the wait can end.** Already rejected in `msgq-completion`: it
  puts words in the agent's mouth.
