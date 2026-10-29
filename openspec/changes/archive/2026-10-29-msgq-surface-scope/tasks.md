## 1. The contract, written once

- [x] 1.1 Write `PROTOCOL.md`: the envelope, the known types and the tolerance rule for unknown ones, the `meta` keys (`ack`, `trace`), the event kinds (`ack`, `presence`), the four address conventions, the offset and cursor rules, and the compatibility policy (additive keeps `v`; unknown field preserved; unknown type delivers as content; `v` bumps only for breaking changes). State plainly which parts are promises and which are conventions. Verify every requirement in `specs/msgq-protocol/spec.md` has a matching paragraph, and that the document names all seven envelope keys.
- [x] 1.2 Add `protocol/fixtures/*.json`: a minimal record, a `prompt` carrying `reply_to`, one record per acknowledgement phase, a presence record, an unknown type, an unknown `meta` key, a `v: 2` record, and a payload that is not an object. Name each file for the behaviour it pins. Verify each file is one JSON document and the set covers every row of the tolerance table.
- [x] 1.3 Make the CLI read the fixtures: a test that decodes each one and asserts accept/reject, including the reason text for `v: 2`. Verify with `cargo test --manifest-path cli/Cargo.toml`.
- [x] 1.4 Make the extension read the same fixtures, and add `command` to `KNOWN_TYPES` in `extension/src/record.ts` so the two lists agree. Verify with `node --test extension/test/`, and confirm that removing `command` from one list and not the other fails a suite.
- [x] 1.5 Add the conformance step and the non-pi canary to the test block in `DEVELOPMENT.md`: both suites, the fixtures, and `scripts/gitlab-watch.sh --to x --once --dry-run` as the client that is not pi. Verify the document lists all three.

## 2. Presence as records

- [x] 2.1 Publish presence as records from the extension: an `event` record with `kind: "presence"`, a state, and a declared lifetime, to `group/presence`; renew on the existing lifetime/3 timer; publish a retraction on teardown. Keep the `start` / `renew` / `setState` / `setOwner` / `stop` shape so call sites barely move. Verify with `node extension/scripts/e2e.mjs presence` and by reading the presence address back with `msgq read group/presence`.
- [x] 2.2 Remove the lease machinery: delete `Lease`, `acquireLease` and the heartbeat path from `extension/src/consumer.ts`, and the lease-name vocabulary from `presence-text.ts`, once nothing calls them. Verify no `heartbeat`, `tryLock` or `msgq:participant:` remains under `extension/src`, and that `node --test extension/test/` passes after the rewrite.
- [x] 2.3 Keep presence non-fatal by construction: a failed publish SHALL NOT throw, SHALL NOT be reported as registered, and SHALL be retried at the next renewal. Verify with the two existing "bus write fails" tests adapted to publishing, plus `node extension/scripts/e2e.mjs presence-off` and `bad-bus`.
- [x] 2.4 Derive the roster in the CLI from presence records: newest record per address, live while younger than its declared lifetime as judged by the stream row's timestamp, retraction honoured immediately, one entry per address using the existing state rank. Keep the `sessions` output and `--json` field set unchanged. Verify with CLI tests over live, expired, retracted, duplicated-alias and empty cases.
- [x] 2.5 Keep the roster cheap on a large presence address: read only what is needed to find the newest record per address rather than scanning the stream. Verify `msgq sessions` answers in well under a second on a presence address seeded with 10k records, and that it does not read the whole stream.
- [x] 2.6 Prove it from outside pi: add an end-to-end scenario in which a non-pi process publishes a presence record with nothing but the CLI, and the roster lists it beside the sessions. Verify with `node extension/scripts/e2e.mjs` for the new scenario and for `presence`, `presence-off`, `crash-expiry`, `rename`, `squat`, `companion-list`.
- [x] 2.7 Confirm the launcher still works untouched: `companion spawn <name> --dry-run` prints the same commands, and a spawned companion still becomes reachable by waiting on `msgq sessions --json`. Verify with the dry run and one real spawn where herdr is available.

## 3. Retention and cursor collection

- [x] 3.1 Add `msgq prune --older-than <age>`: report what would go by default, remove only when told to proceed, and report per-stream counts. Verify with CLI tests over a bus with records on both sides of the age, and a manual run on a copy of a real bus.
- [x] 3.2 Refuse to remove a record at or below any consumer's cursor, and say so. Verify with a test where a cursor sits behind records older than the age: they are kept and named in the report.
- [x] 3.3 Preserve the newest presence record of every address regardless of age, so a participant still ages out on its own terms. Verify with a test where every presence record is older than the age.
- [x] 3.4 Add cursor collection for participants that are not live: remove their cursors, keep a live participant's, and report how many were removed. Verify with a test holding one live and one dead address.
- [x] 3.5 Extend `msgq db info` with per-consumer lag, per-stream oldest-record age, and the presence address's age. Verify the numbers move as expected when a consumer stops consuming, and that the existing output stays parseable.
- [x] 3.6 Exercise pruning and collection together on a bus with live and dead participants, before and after a `db info`. Verify with an end-to-end scenario and a manual check that the database shrinks.

## 4. The launcher as showcase

- [x] 4.1 Write the launcher contract into `companion/README.md`: what a launcher must start, name, tell the companion to report to and trust, pass through, wait for, and stop — plus one line saying herdr is one implementation and a tmux or a harness could satisfy it. Verify the document names every flag and environment variable the script passes.
- [x] 4.2 Move the launcher out of the bus's identity in `README.md`: the front door shows the two-companion exchange, the launcher is described as optional, and the requirements table no longer implies herdr is needed for anything but the launcher. Verify the table has no herdr row and the demo is in the first screen.
- [x] 4.3 Promote `companion spawn <name> --dry-run` in the documentation as the readable form of the contract, so a reader without herdr can see exactly what a launcher runs. Verify the dry-run output is quoted in `companion/README.md`.
- [x] 4.4 Split the client description in `extension/README.md`: the bus client (`peers` and both skills, no herdr) and the launcher client (`companions`, tabs, herdr). Verify each section names the right tools and dependencies.
- [x] 4.5 Update the purpose of the `msgq-companions` capability in the main spec to say the launcher is one implementation of a contract, since its purpose still presents herdr as the definition. Verify with `openspec validate`.

## 5. Documentation and full verification

- [x] 5.1 Revise the presence sections of `extension/README.md`: records on the presence address, the declared lifetime, retraction, and the fact that the extension now appends to one stream. Verify no sentence still describes presence as an engine lease.
- [x] 5.2 Revise `cli/README.md`: the roster is derived from presence records, and the retention and collection commands exist. Verify the "no retention" non-goal is gone or rewritten, and that `sessions`, `prune`, `consumers` and `db info` are documented as they now behave.
- [x] 5.3 Note the boundary in `DEVELOPMENT.md` and the skills if any of them claims otherwise: the wire promises what `PROTOCOL.md` says, and everything else in the repository is a client or a showcase. Verify no skill still describes presence as a lease.
- [x] 5.4 Run the CLI suite and the extension unit suite, plus formatting and lints, and verify all pass: `cargo test --manifest-path cli/Cargo.toml`, `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `node --test extension/test/`.
- [x] 5.5 Run every end-to-end scenario, existing and new, and verify all pass: `node extension/scripts/e2e.mjs` per scenario, including `presence`, `presence-off`, `crash-expiry`, `rename`, `squat`, `companion-list`, `skip-version`, `command-without-herdr`.
- [x] 5.6 Run `openspec validate --strict` for this change and verify it passes.
