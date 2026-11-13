## 1. An answer that is owed

- [x] 1.1 Track the question currently owed an answer: the most recent record that asked for one, and whether a reply has gone out for it since. Verify with unit tests over a question, an answer, a declined answer, and a record that asked for nothing.
- [x] 1.2 Deliver the entry when a run is about to stop with that question still unanswered, naming both outs. Verify with an end-to-end run in which a peer is asked a question it does not answer and then answers after being asked once.
- [x] 1.3 Bound it: once per question, never after an answer, and never for a record that asked for nothing. Verify with an end-to-end run that no second entry is delivered, and a unit test over the answered case.
- [x] 1.4 Report that it fired, where the session has a UI. Verify by running the case and observing the report.

## 2. Completion is reported to the sender

- [x] 2.1 Ask for a completion acknowledgement on what the model sends. Verify with unit tests over the built command line, and a check that a reply, an ask and a list are unchanged.
- [x] 2.2 Verify it end to end: a peer that finishes without answering, and a peer that never runs the message at all, and a peer that answers normally.

## 3. The skill

- [x] 3.1 Teach the other half of a delegation in `msgq-delegate`: a delegation ends in a signal, "nothing to report" is a legitimate ending, and an answer is owed even when the work seemed unimportant. Verify each point is present in the file and that the skill still loads without a diagnostic.

## 4. Documentation and full verification

- [x] 4.1 Document the nudge, the completion report, and the revision of "silence is silent" in `extension/README.md`. Verify the document names all three.
- [x] 4.2 Update the same sentence in the `msgq-notify` skill and in its design note, which currently promise that nothing will ever report a peer that did not answer. Verify neither still claims it.
- [x] 4.3 Run the CLI suite and the extension unit suite, and verify both pass.
- [x] 4.4 Run every end-to-end scenario, existing and new, and verify all pass.
- [x] 4.5 Run `openspec validate --strict` for this change and verify it passes.
