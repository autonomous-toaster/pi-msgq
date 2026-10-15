## 1. The command type on the wire

- [x] 1.1 Accept `command` as a record type in the CLI, alongside the existing five, and let `publish` carry it. Verify with a unit test over the known types, an integration test publishing one and reading it back, and a check that the other five still validate.

## 2. Command delivery

- [x] 2.1 Decide it in the pure delivery plan: a command record from a trusted sender is planned as its own outcome, never as a buffered record, and from an untrusted sender as display-only. Verify with unit tests over all three cases plus the busy case.
- [x] 2.2 Deliver it as input: a note naming the sender and the receiving address, then the body dispatched exactly as if typed. Verify with an end-to-end run in which a peer runs a prompt template sent to it over the bus, and with a check that the note precedes the command in the transcript.
- [x] 2.3 Confirm a command is never coalesced: published while an agent run is in progress, it is queued rather than gathered into a batch. Verify with an end-to-end run and an assertion that no batch entry was produced.
- [x] 2.4 Confirm an untrusted command does nothing: it is delivered display-only and nothing runs. Verify with an end-to-end run using the trust configuration and acknowledgement patterns already used for other turn-bearing records.

## 3. The model's surface

- [x] 3.1 Let the peers tool send a command, so the record's type is `command` and the body is the command line. Verify with unit tests over the built command line.
- [x] 3.2 Refuse to ask a command, with a reason that names the fix. Verify with a unit test over the refusal, and a check that the refusal explains where the request for an answer belongs.

## 4. The skill

- [x] 4.1 Teach it in `msgq-delegate`: when to invoke rather than describe, that the body must be the command line and nothing else, that the task and the request for an answer go in the command's arguments, and that a command runs whatever a typed one runs. Verify each point is present in the file and that the skill still loads without a diagnostic.

## 5. Documentation and full verification

- [x] 5.1 Document the `command` type, dispatch delivery, and what it means for a trusted sender in `extension/README.md`, including a row in the delivery table. Verify the document names the type and the escalation.
- [x] 5.2 Document `--type command` and what a command record is in `cli/README.md`, including the type mapping table. Verify both.
- [x] 5.3 Run the CLI suite and the extension unit suite, and verify both pass.
- [x] 5.4 Run every end-to-end scenario, existing and new, and verify all pass.
- [x] 5.5 Run `openspec validate --strict` for this change and verify it passes.
