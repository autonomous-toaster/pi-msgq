## Why

The bus is fire-and-forget: a sender cannot tell whether a session received, started, or finished
a message. Publishing is blind, and the multi-agent work ahead has no way to know that a delegated
task began or completed. The pieces are already there — `reply_to` has been in the record contract
since the beginning and has never been written, and the extension already observes run boundaries
(`agent_start`, `agent_settled`) — so acknowledgements are the missing half of the conversation.

## What Changes

- Add **acknowledgements** as ordinary bus records: `type: "event"`, `event.kind: "ack"`,
  `reply_to` set to the original record's offset, published to the sender's own address.
- Define three phases — `received`, `started`, `completed` — where `received` reports **how** the
  message was delivered and is terminal for a display-only delivery, and `completed` reports an
  **outcome**.
- Let a sender opt in per message with `meta.ack`; unacknowledged messages produce nothing.
- Add `msgq send --await` and `msgq await <offset>` to the CLI: publish, then print phases as they
  arrive and exit on a terminal phase. Add a distinct exit status for a timeout.
- **BREAKING (behavioral):** the extension now saves the consumer cursor when it *takes
  responsibility* for a message rather than when it is delivered. An interruption therefore
  redelivers work that was not yet acted on, instead of silently dropping it. The replay guarantee
  moves from "at most one record" to at-least-once.
- Widen the extension's scoped write exception from *leases only* to *leases plus
  acknowledgements*. It still never publishes an arbitrary message.

Explicitly not in this change:

- No result payload in an acknowledgement.
- No resumption: an acknowledgement never starts a turn. Notification only.
- No progress phase and no cancellation.
- No new record type, no schema change, no protocol version bump.
- No change to acknowledgement trust: acks are unverified, exactly like `from`.

## Capabilities

### New Capabilities

<!-- None. Acknowledgements extend both existing capabilities. -->

### Modified Capabilities

- `msgq-extension`: add the acknowledgement lifecycle, acknowledging addressing and resilience;
  change cursor durability to responsibility-based saving; widen the scoped write exception.
- `msgq-cli`: add awaiting acknowledgements; add a timeout exit status.

## Impact

- `extension/`: acknowledgement emission at the three hooks, responsibility-based cursor saving,
  and capture of the run outcome.
- `cli/src/`: the `await` surface, a reply-stream read, phase rendering, and the new exit status.
- Both component READMEs.
- No bus schema change, no migration. Acknowledgements are ordinary records, so older readers
  already tolerate them as `event` records and ignore them.
- `meta.ack` becomes an extension-read convention living in the free-form `meta` bag. This is a
  deliberate, recorded trade-off: it keeps the record contract stable while the acknowledgement
  shape is still moving, at the cost of a protocol instruction living in a bag documented as never
  read by the bus.
