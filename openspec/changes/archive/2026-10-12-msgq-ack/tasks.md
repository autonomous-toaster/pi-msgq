## 1. Extension — acknowledgement records

- [x] 1.1 Add an acknowledgement builder that produces the record envelope (`type: event`, `event.kind: ack`, phase, and `reply_to` set to the original offset, addressed to the sender); verify with unit tests covering each phase and the display-mode terminal shape.
- [x] 1.2 Parse the acknowledgement request from `meta.ack` (accepting `true` for all phases and a comma-separated phase list); verify unit tests for all-phases, a subset, an absent request, and an unrecognised value.
- [x] 1.3 Emit `received` when the extension takes responsibility for an acknowledged record, carrying the delivery mode, and suppress any later phase for a display delivery; verify that a display delivery produces exactly one acknowledgement.

## 2. Extension — started and completed

- [x] 2.1 Track acknowledged records that are in flight and emit `started` when a run begins; verify an acknowledged prompt produces `received` then `started`.
- [x] 2.2 Emit `completed` with the run outcome when the agent settles; verify the outcome reflects a normal run and an aborted one.
- [x] 2.3 Emit `started` and `completed` for every acknowledged record in a coalesced batch, each correlated by its own offset; verify with several acknowledged records buffered during a run.
- [x] 2.4 Ensure a terminal acknowledgement is the last one for a record; verify no further phase follows `completed` or a display-terminal `received`.

## 3. Extension — cursor durability

- [x] 3.1 Stop the binding's automatic cursor save and persist the offset explicitly when responsibility is taken; verify the cursor does not advance at delivery.
- [x] 3.2 Persist after a display record is shown and after a record starts a turn; verify the offset has advanced once each has been acted on.
- [x] 3.3 Persist when a coalesced batch is injected; verify the offset covers the whole batch only after injection.
- [x] 3.4 Verify that a stop between delivery and responsibility redelivers the record on the next start, and that nothing is silently dropped.

## 4. Extension — resilience and anti-loop

- [x] 4.1 Make a failed acknowledgement publish non-fatal and report it where the session has a UI; verify consumption continues when publishing fails.
- [x] 4.2 Never acknowledge a record that cannot be decoded; verify a malformed payload emits no acknowledgement and consumption continues.
- [x] 4.3 Ensure an acknowledgement never starts a turn and is never itself acknowledged; verify with an acknowledgement published to a session under the honour policy.

## 5. CLI — awaiting acknowledgements

- [x] 5.1 Add `msgq await <offset>` and `msgq send --await`, recording the reply stream's position before publishing; verify an acknowledgement emitted immediately after publish is still reported.
- [x] 5.2 Report each acknowledgement as it arrives and finish on a terminal phase; verify a started/completed sequence is printed in order and the command exits 0.
- [x] 5.3 Add `--json` output emitting one object per acknowledgement; verify each line parses and carries the phase, reply reference, and sender.
- [x] 5.4 Add a wait window (`--timeout`, default 300s, `0` unbounded) and exit status 3 on timeout; verify a timeout exits 3 and an unbounded wait does not stop early.

## 6. Verification

- [x] 6.1 End-to-end, display policy: publish an acknowledged message and confirm the only acknowledgement is `received` with a display delivery, and the wait ends there.
- [x] 6.2 End-to-end, honour policy: confirm `received`, `started`, and `completed` arrive in order for a prompt that runs.
- [x] 6.3 End-to-end, coalesced: confirm a batch of acknowledged records yields one `started` and one `completed`.
- [x] 6.4 End-to-end durability: confirm a turn-bearing record buffered during a run and then interrupted is redelivered on the next start rather than lost.
- [x] 6.5 Confirm no acknowledgement is emitted for records that do not request one, and that acknowledgements never start a turn.
- [x] 6.7 Render acknowledgements as lifecycle lines in human-readable `read` and `follow` output, leaving JSON output unchanged; verify with an integration test that a seeded acknowledgement prints a lifecycle line.
- [x] 6.8 Let `msgq publish --ack` request acknowledgements from shorthand input; verify an integration test that the published record carries the request.
- [x] 6.9 Derive the reply address in `msgq await` from the sender recorded on the referenced message; verify an integration test that a wait without an explicit address finds the acknowledgement.
- [x] 6.6 Update both component READMEs with the acknowledgement phases, the `meta.ack` request, and the await surface, and verify the documented behaviour.
