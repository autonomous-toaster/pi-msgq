# Tasks

## 1. Key material

- [x] 1.1 Add `ed25519-dalek`, `rand` and `sha2` to `cli/Cargo.toml` and verify `cargo build` succeeds.
- [x] 1.2 Add a key module that writes a principal's private key to `~/.config/msgq/keys/<principal>.key` with mode 0600 and verifies the mode with a test on the written file.
- [x] 1.3 Derive the public key and an OpenSSH-style `SHA256:` fingerprint from the private key, and verify a unit test matches a fixed key's known fingerprint.
- [x] 1.4 Add `msgq keygen` and `msgq fingerprint`, with no bus required, and verify an integration test runs both against a temporary `HOME` and asserts that no bus file is created.
- [x] 1.5 Verify no command prints private key material by asserting the private bytes never appear in the output of any of these commands.

## 2. Signing on publish

- [x] 2.1 Add the `meta.sig` object to the record vocabulary in `cli/src/record.rs`, preserving it through parse and serialize, and verify an existing round-trip test covers a record carrying `meta.sig`.
- [x] 2.2 Implement the domain-separated signed-bytes encoding of design.md, and verify unit tests show two spellings of one sender produce identical bytes and that changing `from`, `to`, `type` or body each changes the bytes.
- [x] 2.3 Sign on publish when a key exists for the principal, and verify a test publishes and asserts `meta.sig` verifies against the printed fingerprint.
- [x] 2.4 Create a key lazily on first publish and report the fingerprint and its location, and verify a test on a fresh `HOME` asserts the notice is printed and the key file exists.
- [x] 2.5 Add signed fixtures to `protocol/fixtures/` — a valid signature, a tampered body, a restamped sender, an unknown algorithm, and a mixed stream — and verify the conformance test accepts and rejects the same fixtures as before plus these.

## 3. Verification and verdicts

- [ ] 3.1 Implement verification over the signed bytes against a supplied anchor, and verify unit tests cover valid, broken, and unknown-algorithm signatures.
- [ ] 3.2 Compute a verdict per record in the read and follow paths and carry it in every output mode, and verify a test reads a mixed stream and asserts each record's verdict appears in text and JSON output.
- [ ] 3.3 Refuse unsigned records by default unless `MSGQ_INSECURE=1` is set, and verify tests assert the default fails with an error naming the record and the setting, and that one invocation with the variable succeeds.
- [ ] 3.4 Report a refusal with the principal, the fingerprint and the reason, and verify the error text names all three.

## 4. The trust keyring

- [ ] 4.1 Derive a bus id from a file path and from a server location using the slug-plus-digest idiom of `cli/src/pg/naming.rs`, and verify a test asserts one bus's id is stable across spellings and differs from another bus's.
- [ ] 4.2 Write and read `~/.config/msgq/trust/<bus-id>/anchors.toml` with current and retired entries, atomically, and verify a test asserts a failed write leaves the previous keyring intact.
- [ ] 4.3 Add the trust verbs — record, revoke, rotate, list — and verify integration tests cover each against a temporary `HOME`.
- [ ] 4.4 Refuse recording one key under two principals and verify the error names the principal that already holds it.
- [ ] 4.5 Implement rotation as moving current to retired and counting the retained records it makes refused, and verify a test rotates and asserts both the count and that an old record is now refused.
- [ ] 4.6 Verify a keyring written by one bus is never read by another by asserting an anchor for a server bus leaves file-bus records unverified.

## 5. The environment layer and posture reporting

- [ ] 5.1 Parse `MSGQ_TRUST` as one or more `principal:fingerprint` entries and verify tests cover one entry, several entries, a malformed entry failing the run, and a contradictory pair failing the run.
- [ ] 5.2 Resolve anchors with the precedence environment over durable, reporting a disagreement naming both fingerprints and both layers, and verify a test asserts the environment anchor wins and the disagreement is reported.
- [ ] 5.3 Assert an environment anchor writes no file by running once with the variable and once without, and verifying the second run trusts nothing.
- [ ] 5.4 Extend the bus description with the trust posture: unsigned refused or permitted, layers in force, the bus id, and the count of records from unverified senders, and verify the report names each.
- [ ] 5.5 Document the trust surface in the CLI reference — the three layers, the precedence, `MSGQ_TRUST`, `MSGQ_INSECURE`, and what rotation does — and verify every documented command runs as written.

## 6. The extension surface

- [ ] 6.1 Register `/msgq-trust` as a command and verify it is absent from the tool list by asserting the registered tools do not include it.
- [ ] 6.2 Implement status, defaulting to the durable layer, with a session-only form, and verify command tests cover recording durably, recording for the session, and rendering both layers with the bus named.
- [ ] 6.3 Keep the session layer in memory only and verify a test asserts a fresh session reports none of a previous session's anchors.
- [ ] 6.4 Render the verdict of every delivered record, showing an unverified record as unverified and a refused one as refused with its reason, and verify render tests cover trusted, unverified, broken and retired cases.
- [ ] 6.5 Count records from unverified senders in the session and report the count in status, and verify a command test delivers two such records and asserts the count.
- [ ] 6.6 Assert that a delivered record asking for trust, or carrying a key to adopt, writes no anchor, by a test that publishes such a record and checks the keyring is unchanged.
- [ ] 6.7 Document the command in `extension/README.md`, and verify the documented invocations match the registered command's behaviour.

## 7. Integration

- [ ] 7.1 Run the end-to-end scenarios and verify a signed publish reads as trusted on a peer that pinned the fingerprint, as unverified on one that did not, and as refused after rotation.
- [ ] 7.2 Verify the same scenarios pass against both storage engines, per the conformance requirement.
- [ ] 7.3 Correct the `sender_of` comment in `sql/authorization.sql` so it no longer claims a session cannot speak as a sibling, and verify the spec wording and the comment agree.

## Workflow follow-up

- Archive the change after the project's review requirements are satisfied.
- Verify the archived result.
