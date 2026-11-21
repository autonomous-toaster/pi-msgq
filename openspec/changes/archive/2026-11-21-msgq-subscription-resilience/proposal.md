# Proposal

## Why

On 2026-11-21 a peer session spent eleven hours publishing to the bus while receiving
nothing. Its consumer cursor stayed at one offset while 26 records (10 messages, 16 acks)
piled up unread; its peer, holding the address, kept writing into a stream nobody read. The
session had no way to find out: the only report of the failure was a transient UI toast, so
neither the model nor the human saw it, and it went on answering a conversation it could no
longer hear. Recovery happened by accident — the session read its own backlog by hand with
`msgq read --from-start`.

Two defects combine to produce that. A single throw on the delivery path calls `disable()`,
which tears the extension down for the rest of the process: no retry, no re-attach, no second
attempt. And a resumed session that fails to attach never tries again, so the durable cursor
keeps the records but nothing ever replays them. A message bus whose receive path can die
silently, while the send path keeps working, turns a two-way conversation into a one-way
broadcast that looks healthy from both ends.

## What Changes

- A failure on the delivery path stops being terminal. A transient store error retries with
  backoff, keeps the cursor, and rebuilds the subscription; only a configuration error (an
  unknown policy, an address whose scheme does not exist) still disables the extension.
- Attachment becomes a state that is rebuilt, not an event that either succeeds or kills the
  session. The extension re-attaches on session start, after a failure, and at the latest on
  the next turn boundary, from the offset it had already persisted.
- A loss and a recovery are written into the transcript as msgq records, not only to the UI
  toast: a detached session says so where the model reads, and a recovered one says so before
  it resumes. The status slot reports `attached`, `detached (retrying)` or `disabled`.
- A re-attach announces what was waiting: one line naming the number of records replayed from
  the durable cursor, before those records are delivered.
- A session that cannot receive stops claiming to be reachable: presence is not renewed while
  detached, because a roster entry that promises delivery is a lie the peer acts on.
- Teardown is split: releasing resources for shutdown, and rebuilding a broken subscription,
  stop being the same operation, so one can happen without the other.
- Non-goals: no change to the backend contract, the record envelope, the delivery policy, or
  the acknowledgement lifecycle; no supervisor process or daemon watching the extension from
  outside; no change to how a record is rendered.

## Capabilities

### New Capabilities

None. The behavior belongs to the extension, which already owns delivery, lifecycle and
degradation; a separate capability would duplicate its requirements.

### Modified Capabilities

- `msgq-extension`: `Graceful degradation` no longer treats an unreadable bus as terminal;
  `Cursor durability` requires a re-attach to replay from the saved offset and report what it
  replayed; `Loading and lifecycle` requires attachment to survive a failed subscription;
  `Presence registration` stops a detached session from presenting itself as reachable; and a
  new requirement states that loss and recovery are visible in the transcript.

## Impact

- `extension/msgq.ts`: `disable()`, `teardown()`, `report()`, `setStatus()`, `session_start`,
  `resubscribe()`, and the turn-boundary hooks that can carry a retry.
- `extension/src/consumer.ts`: the pump loop and the subscription lifetime — a cursor that
  survives an error, and a subscription that can be rebuilt without losing the offset.
- `extension/test/consumer.test.ts` and `extension/test/edge.test.ts`: the error path is
  currently only asserted to disable; it will assert recovery instead.
- Observability: a session's attachment state becomes readable in the transcript and in the
  status slot, which is what makes this class of failure diagnosable in the field.
