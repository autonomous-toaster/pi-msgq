## 1. The record id in the CLI

- [x] 1.1 Add a pure helper that forms a record id from an address and an offset, and its inverse. Verify with unit tests over a plain address, an address that already contains `@`, and a malformed id.
- [x] 1.2 Let `publish` report the id instead of the bare offset, leaving the default output unchanged. Verify with an integration test that the default still prints only the offset, and that asking for the id prints `<address>#<offset>`.
- [x] 1.3 Let `--reply-to` accept an id as well as a bare offset, refusing an id whose address is not the record's own sender. Verify with integration tests for the accepted case, the bare-offset case, and the refused case writing nothing.

## 2. Reference annotation in the extension

- [x] 2.1 Add a one-shot read to the extension's bus wrapper: one record at a given offset in a named stream, without subscribing. Verify with a test asserting the record is returned, and that no consumer cursor moves.
- [x] 2.2 Add a pure formatter for the annotation: given a resolved record or nothing, produce the line that names the id and quotes the record. Verify with unit tests for the quoted case, the unreadable case, and a record whose body is empty.
- [x] 2.3 Resolve and annotate at delivery, for any record carrying a reply reference, whatever its type. Verify with unit tests over the content, and an end-to-end run where a referencing record arrives and the delivered content names the id and quotes the record it refers to.
- [x] 2.4 Confirm the annotation changes nothing else: it does not acknowledge, redeliver, or move a cursor, and a record that references nothing is delivered exactly as before. Verify by re-running the acknowledgement and delivery scenarios unchanged.

## 3. The shipped skill

- [x] 3.1 Add the `pi` manifest to `extension/package.json`, declaring both the extension entry and the skill directory, and add `extension/skills/msgq-notify/SKILL.md`. Verify that `pi -e ./extension` still loads the extension, and that the skill is advertised as `skill:msgq-notify`.
- [x] 3.2 Write the skill's instructions so that it covers reaching a session from outside: finding this session's own address; publishing to it to wake it; that `--from` must not be this session's own address, because it would never start a turn; the default sender and what a trust list does to it; which record type to use for a result arriving mid-run; `--ack` and `--await` as a receipt; and that nothing reports a peer that never answered. Verify that each of those points is present in the file, and that the skill loads without a diagnostic.
- [x] 3.3 Confirm a missing or malformed skill does not take the extension with it. Verify by pointing the manifest's skill path at nothing and running a delivery scenario: the extension still loads, consumes, and delivers.

## 4. End to end

- [x] 4.1 Add a scenario where a record arrives carrying a reply reference that can be read, and verify the delivered content names `<from>#<offset>` and quotes the referenced record.
- [x] 4.2 Add a scenario where an acknowledgement carrying a reply reference arrives, and verify it is annotated the same way as a reply.
- [x] 4.3 Add a scenario referencing an offset that is not in the stream, and verify the content names the id without a quote and delivery continues.
- [x] 4.4 Add a scenario that walks the skill's documented pattern as a shell user would: a process that is not the session publishes to it with a turn-bearing type from a sender it trusts, and verify the session is woken.

## 5. Documentation and full verification

- [x] 5.1 Document the record id, the reference annotation, and the shipped skill in `extension/README.md`. Verify the document names all three and matches the observed behavior.
- [x] 5.2 Document the id, the reporting flag, and referencing by id in `cli/README.md`. Verify the document shows the id and both forms of `--reply-to`.
- [x] 5.3 Document the manifest and the trap it sets in `DEVELOPMENT.md`: that the manifest must re-declare the extension entry, and how to check that the skill loaded. Verify the section names the trap and the check.
- [x] 5.4 Run the CLI suite and the extension unit suite, and verify both pass.
- [x] 5.5 Run every end-to-end scenario, existing and new, and verify all pass.
- [x] 5.6 Run `openspec validate --strict` for this change and verify it passes.
