# Tasks

## 1. Wire amendments and conventions (backend-neutral)

- [x] 1.1 Amend `PROTOCOL.md`: offsets are unique within a stream; a record's identity is `(address, offset)`; a reply reference is an offset in the stream named by the answering record's `from`. Verify by reading the Streams/offsets section and confirming `openspec validate pgmq-backend` still passes.
- [x] 1.2 Amend `PROTOCOL.md` retention: replace "no policy is implied" with the declared window, and state that a consumer behind the window loses records. Verify the Authentication and "What is not promised" sections name the two trust statements (file permissions, credential).
- [x] 1.3 Amend `PROTOCOL.md` for the readable/opaque boundary: envelope, presence and acknowledgement bodies are always readable; `message`/`event` content may be opaque; document `meta.enc` (`alg`, `kid`) and `meta.reply`. Verify a v1 reader paragraph exists for each and that no existing requirement was reworded.
- [x] 1.4 Add fixtures for the new conventions to `protocol/fixtures/`. Include a record with `meta.enc`. Include a record with `meta.reply`. Include an opaque body. Update `expectations.json` for the new fixtures. Verify `cargo test --test cli` against `expectations.json`. Verify `node --test test/` in `extension/` against the same file.
- [x] 1.5 Teach the SQLite reader path that an unreadable body is an error, not content, and that presence/ack bodies are never treated as opaque. Verify a unit test in `cli/src/presence.rs` and `extension/test/delivery.test.ts` exists for both cases.

## 2. Retention window on the local engine

- [x] 2.1 Add retention resolution (`MSGQ_RETENTION`, `--retention`, default `1h`) beside the existing location resolution in `cli/src/config.rs`. Verify unit tests cover the default, the environment value, the flag, and a malformed value.
- [x] 2.2 Rewrite `prune` in `cli/src/commands.rs`: drop the "refuse when a consumer has not passed it" rule, keep the dry-run default and the newest-presence pin, and report which consumers will lose records. Verify `cli/tests/cli.rs` covers: old records removed, a lagging consumer named but not obeyed, presence kept, nothing removed without `--apply`.
- [x] 2.3 Implement the window as the default for `prune`. Add batched removal through the store boundary. Verify a test with a few thousand pruned records completes without a single unbounded statement (assert batch count) and reports the total.
- [x] 2.4 Add gap detection: `read` and `follow` report how many records were lost, `consumers` reports lag and dropped counts, `--strict` makes a read exit non-zero. Verify tests for each of the three commands plus the `--strict` exit code.
- [x] 2.5 Extend `db info` with backend kind, retention window, and credential state (absent on the local engine). Verify a test asserting all three fields appear.
- [x] 2.6 Update `README.md` (retention, the `await` addressing change). Verify every documented command in the changed sections runs as written against a local bus.
- [x] 2.7 Address `await` by record id, and refuse a bare offset with no `--on`. Verify `cli/tests/cli.rs` covers the id form, the offset-plus-address form, both given, and an id naming no record.
- [x] 2.8 Update `DEVELOPMENT.md` with the two-engine story.

## 3. Store boundary

- [x] 3.1 Extract the store trait in `cli/src/store.rs` (`ensure_address`, `publish`, `read_since`, `head`, `tail`, `cursor_get/set/reset`, `prune_before`, `list_addresses`, `list_consumers`, `wake`, plus the retention methods added in group 2) and move the honker implementation behind it. Verify the full existing CLI test suite passes unchanged.
- [x] 3.2 Add backend selection to `cli/src/config.rs`: a server connection string selects the shared engine, anything else is a path, an unsupported scheme is a usage error. Verify unit tests for all three, including that no file is created for a connection string.
- [x] 3.3 Introduce the same interface in `extension/src/` with the existing honker backend behind it. Verify `extension/test/consumer.test.ts` passes and the extension still delivers end to end via `extension/scripts/e2e.mjs`.
- [x] 3.4 Add the behavioural fixture family (`protocol/fixtures/behaviour/`) and a runner for the local engine covering ordering, cursor monotonicity, non-destructive reads, retention, gap reporting, and address round-tripping. Verify the runner passes on the local engine and reports the shared engine as not yet supported.

## 4. Shared engine: storage and delivery

- [x] 4.1 Add the Postgres store skeleton. Connect to the engine. Bootstrap `msgq.address` and `msgq.cursor`. Record `protocol_version`, `created_by` and the pgmq version. Verify against a local Postgres that a fresh connection creates only the bus's own tables and touches no `pgmq` table.
- [x] 4.2 Implement address-to-queue naming (slug plus digest, under the engine's length limit) with the registry and the `headers` address stamp. Verify round-trip tests for a long `cwd/` address, a case-differing pair, and two addresses whose slugs collide.
- [x] 4.3 Implement `ensure_address` and `publish` with lazy, idempotent creation outside the record transaction. Verify that two concurrent first publishes both succeed, that a read of an unpublished address creates nothing, and that the second publish costs no DDL.
- [x] 4.4 Implement `read_since`, `head` and `tail` as read-only queries over the engine's queue tables. Confirm that nothing mutates on read. Verify by comparing a per-record counter and a row checksum before and after two identical reads.
- [x] 4.5 Implement cursors on `msgq.cursor` with monotonic semantics. Verify a cursor write with a lower value is ignored and that a read never advances one.
- [x] 4.6 Implement `wake` with the engine's insert notification, throttled per queue, falling back to polling when the connection cannot listen. Verify a latency test (wake under a second) and a forced-fallback test (polling still delivers).
- [x] 4.7 Implement `prune_before` for the shared engine. Create the `enqueued_at` index on each queue. Delete in batches. Keep the newest presence record per participant. Verify on a queue with a large old prefix that the total is reported and the presence pin survives.
- [x] 4.8 Implement `list_addresses` and `list_consumers` from the engine's queue registry plus the bus's own registry and cursor table, including addresses whose records have aged out. Verify `topics` shows a zero-count address and `consumers` shows lag and dropped counts.
- [x] 4.9 Run the behavioural fixture family against the shared engine. Make the runner fail rather than skip. Verify both engines produce identical outcomes for every fixture.

## 5. Authorization on the shared engine

- [x] 5.1 Add the identity table and credential verification with `gen_random_bytes(32)` and `digest(token,'sha256')`. Read the credential from the identity file or `MSGQ_TOKEN`. Place no credential inside a record. Verify a test that publishes and then asserts no field of the stored record contains the token or its digest.
- [x] 5.2 Add the authorizing publish function (`SECURITY DEFINER`, `SET search_path = pg_catalog, pg_temp`) that resolves the credential, checks the address against scopes, stamps `from`, creates the queue when absent, and sends the record. Verify in scope, out of scope, expired, and missing-credential cases each produce the specified outcome and leave the bus unchanged when refused.
- [x] 5.3 Revoke direct write access (`CREATE` on the engine schema, `INSERT` on queue tables) from the runtime role so the function is the only write path. Verify a direct insert attempt as the runtime role is refused.
- [x] 5.4 Make the CLI report a refused or missing credential as an error with no unauthenticated retry, and report credential state in `db info`. Verify `cli/tests/cli.rs` covers refusal (non-zero exit, nothing written) and that no fallback path exists.
- [x] 5.5 Scope presence to a per-scope address in both the CLI roster and the extension's registration. Verify a test where two scopes publish presence and each roster lists only its own.

## 6. Extension on the shared engine

- [ ] 6.1 Implement the extension's shared-engine bus (`readSince`, `saveOffset`, `subscribe`, wake, reconnect) against the same interface as the local bus. Verify `extension/test/` covers a subscribe-and-receive cycle against a local Postgres.
- [ ] 6.2 Add gap handling to the extension consumer: report the dropped count once, resume at the oldest retained record, keep consuming. Verify a test where the cursor is behind a pruned prefix.
- [ ] 6.3 Add the opaque-body rule to delivery: consult `meta.enc` before `type`, report an error, start no turn, and publish no `received` acknowledgement. Verify tests for each of the three consequences.
- [ ] 6.4 Add the distinct degradation states: bus unopenable, credential rejected (read-only, reported), and unsupported record version. Verify `/peers me` distinguishes a read-only session from an idle one.

## 7. Integration

- [ ] 7.1 Run both conformance suites (record shape and behaviour) against both engines in CI. Verify the CI job fails when one engine's behaviour is changed in isolation.
- [ ] 7.2 Update `README.md` with the two-backend table and the migration note for `msgq await`, and confirm each documented command runs against the backend it names.
- [ ] 7.3 Run an end-to-end scenario on the shared engine with two sessions, one `ask` and its reply, a `note`, and presence. Confirm both rosters and the acknowledgements. Verify with `extension/scripts/e2e.mjs` extended for the shared backend.

## Workflow follow-up

- Archive the change after review requirements are satisfied.
- Split message encryption into its own change, reusing the `meta.enc` seam and the readable/opaque boundary established here.
