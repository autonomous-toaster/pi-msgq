## Why

Two sessions can ask each other a question and neither ever answers. The reason is structural rather
than a race:

- `peers({action:"ask"})` blocks the asking session's whole turn in `pi.exec("msgq ask …")`.
- While that run is active the session is not idle, so an arriving question is **buffered** and only
  injected when the run settles — which cannot happen while the tool call is still waiting.
- So the peer's question is unreachable exactly while it matters, and the peer's own `ask` waits for
  an answer this session cannot produce.

Verified against a real bus with two single-threaded session simulators: with `--timeout 0` both
sides wait forever (and the tool offers the model exactly that value, described as "0 waits without a
limit"); with a bounded timeout each side returns only when its own timeout expires and immediately
re-blocks on the same shape, so a two-party exchange makes no progress and a cycle of three costs the
sum of every timeout.

Two more faults were found while verifying it:

- **The answer arrives twice.** A blocking ask returns the reply in the tool result, and the same
  record is *also* delivered by the ordinary bus path once the turn settles, so the model reads the
  answer again as a fresh inbound message and may answer it again.
- **A session can ask itself.** A record from this session to this session is always delivered
  display-only, so a self-ask can never be answered and burns the full timeout.

## What Changes

- **`ask` no longer waits by default.** It publishes the question and returns; the answer arrives as
  an ordinary inbound message that starts a turn, exactly as `send` already works. Nothing holds an
  execution slot across a wait, so no cycle of sessions can deadlock.
- **Waiting is opt-in and bounded.** `wait:true` keeps the one-turn exchange for a peer that is
  plainly idle, with the wait clamped to at most 120 seconds and 0 no longer meaning "forever".
- **A wait is abandoned when the peer asks you something.** While waiting on `session/x`, a question
  arriving from `session/x` ends the wait immediately and tells the model to answer it first. The
  common two-party cycle costs milliseconds instead of minutes.
- **An answer the tool already returned is not delivered again.** The extension knows which record is
  the answer to a wait it just completed, and consumes it instead of re-injecting it.
- **A question to yourself is refused**, like a command that cannot be asked.
- **The CLI gains `ask --no-wait`**, so the non-blocking question is built in the one place that
  knows what a question record looks like. The blocking `ask` keeps its meaning for scripts, where
  holding a process open is the point.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `msgq-extension`: the peer tool's ask no longer blocks by default, its wait is bounded and can be
  abandoned, and an answer is delivered once.
- `msgq-cli`: `ask` gains `--no-wait`.

## Impact

- No wire change: a non-blocking ask is the same `prompt` record with the same `meta`, published by
  the same code path. `PROTOCOL.md` is untouched.
- One new piece of light state in the extension, bounded to one turn: the peer currently being waited
  on, and the peers whose answer was already handed to the model.
- The skill that teaches "block, or be woken" is rewritten: `send` is now the norm and `ask` is the
  short question to a peer that answers at once.
