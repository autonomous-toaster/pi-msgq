## 1. The CLI can ask without waiting

- [x] 1.1 Add `ask --no-wait`: publish the question record and return, printing its offset. Verified by `ask_no_wait_returns_as_soon_as_the_question_is_published`: the command returns with no reply in existence, and the record carries the `ack` and `reply` hints that make it a question.
- [x] 1.2 Verify a blocking `ask` is unchanged, including that the two records are the same one. Verified by `ask_no_wait_publishes_the_same_record_as_a_blocking_ask` (identical records, differing only in offset) and by the existing reply/finished/still-running tests.

## 2. The extension does not wait by default

- [x] 2.1 Plan a non-blocking ask by default and a bounded one for `wait:true`, clamping the wait to 120 seconds and treating 0 as the minimum rather than as no limit. Verified in `tools.test.ts`: the default plan, the waiting plan, and `askWaitSeconds` over 0, −5, 30, 5000, NaN and undefined.
- [x] 2.2 Refuse an ask addressed to this session's own address (and to `me`). Verified in `tools.test.ts`.
- [x] 2.3 Report the two endings in words: an answer returned in the turn, and an outcome (still running, finished without replying). Verified in `tools.test.ts` over the record shape, both outcome shapes, and an unparseable report.

## 3. A wait can be abandoned

- [x] 3.1 Abort the wait when the peer being waited on asks this session a question, and tell the model to answer it first. Verified end to end by `ask-cycle`: a real session waiting on a stubbed peer settled in ~6 s instead of its 120 s window, and its tool result says the peer is asking.
- [x] 3.2 Verify the peer's question is still delivered afterwards, so abandoning the wait loses nothing. Same scenario asserts the question reaches the transcript as a delivered message.

## 4. An answer is delivered once

- [x] 4.1 Consume an inbound record that is the answer to a wait this turn already returned, and report that it was consumed. Verified two ways: `answersConsumedAsk` unit tests (five cases), and `ask-answered-once` end to end — a waiting ask answered by a stubbed peer leaves the answer in the tool result and **no** copy in the transcript. With the consumption disabled the same scenario reports `duplicates=1`, so the scenario can fail.
- [x] 4.2 Verify a wait that ended without an answer suppresses nothing: `interpretAsk` reports no answer for both outcome shapes, so no peer is recorded as answered, and the predicate is unit-tested to refuse when that list is empty. A late answer therefore arrives as an ordinary message.

## 5. Documentation

- [x] 5.1 Rewrite the peer tool's ask in `extension/README.md`, `README.md`, the `msgq-delegate` skill and `cli/README.md`: sending and being woken is the norm, `wait:true` is the bounded short question, and no session waits on another in a cycle.
- [x] 5.2 Verify the suites: `cargo test` (83 passed), `node --test extension/test/` (146 passed), `openspec validate --strict` for this change, and the whole end-to-end set (44 scenarios, three of them new: `ask-wakes`, `ask-cycle`, `ask-answered-once`) — all pass. The new scenarios were also run with `MSGQ_BIN` unset, so they exercise the installed `msgq` rather than the checkout build. `cargo fmt --check` and `cargo clippy -D warnings` still report pre-existing faults in `cli/src/card.rs` and one unformatted block in `cli/tests/cli.rs`; this change neither adds nor fixes them.
