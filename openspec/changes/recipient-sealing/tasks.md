# Tasks

## 1. The key agreement, before anything depends on it

- [x] 1.1 Try `cargo add x25519-dalek --offline`; if it does not resolve, implement RFC 7748 X25519 over the `curve25519-dalek` already in the tree, and verify against the RFC's own test vectors plus the all-zero-output check.
- [x] 1.2 Derive an agreement key from a principal's seed with HKDF-SHA256 under `msgq/agree/v1`, and verify two derivations of one seed differ and one derivation is stable.
- [x] 1.3 Assert the signing key is unchanged by this change: a signature made before it SHALL still verify, by testing against the pinned fixture key.
- [x] 1.4 Derive the agreement public key and put it in `meta.sig.agree` when signing, and verify a record signed before this change still verifies and a record signed after it carries the key.
- [x] 1.5 Verify that altering `meta.sig.agree` on a stored record makes it refused, because the signature covers it.

## 2. The envelope

- [x] 2.1 Seal a body under a fresh content key and wrap that key for each recipient under an ephemeral X25519, and verify two seals of one body differ and each recipient opens it with its own key.
- [x] 2.2 Give each recipient its own ephemeral, and verify two wraps of one record carry different ephemeral public keys.
- [x] 2.3 Bind the derived key to the pair with a salt of `ephemeral_public || recipient_public`, and verify a wrap made for one recipient does not open for another.
- [x] 2.4 Add `meta.enc.aad` with `"v2"` binding `canonical(from)` and `to`, keep `v1` as the meaning of an absent field, and verify a v1 record still opens and a body moved to another address fails under v2.
- [ ] 2.5 Add the fixtures: two recipients, a wrap that does not open, a record sealed to a third party, a v1 record, and one stream holding all of them; verify both the Rust tests and the extension's conformance test accept and reject the same cases.

## 3. `msgq open`

- [x] 3.1 Add `msgq open` reading records on stdin and writing one outcome per record, and verify a record sealed to the reader opens, one sealed to another reports who, one under an unknown algorithm reports the algorithm, and one altered reports that the body does not open.
- [x] 3.2 Return success when a record is sealed to somebody else, and verify the exit status and that no body is printed as content.
- [x] 3.3 Make `read`, `follow` and `await` use the same outcome rules, and verify an unopenable-to-me record prints with its recipient named instead of failing the read.
- [ ] 3.4 Remove the opening half of `extension/src/seal.ts` and call `msgq open` instead, and verify the extension's tests still cover opening through the verb.

## 4. Sealing by rule

- [x] 4.1 Seal a publish to a `session/` or `cli/` address for the recipient and the sender, and verify both can open the record and the sender reads it back through `read`.
- [x] 4.2 Resolve the recipient's agreement key from any record it wrote, and verify a publish to a known peer needs no new lookup and works with the peer offline.
- [ ] 4.3 Fail a publish to a participant whose key is unknown, naming the address and `--plaintext`, and verify the record is not stored.
- [x] 4.4 Keep groups on the bus key and report which key was used, and verify `--seal-to` overrides that for a group.
- [ ] 4.5 Report the sealing mode and the agreement key in force in `db info`, and verify the report names both and prints no key material.

## 5. Key custody and rotation

- [ ] 5.1 Archive the replaced seed as `<principal>.<date>.key.retired` on rotation and report its path, and verify an old record sealed under it still opens afterwards.
- [ ] 5.2 Report the archived path in the rotation output, and verify the report names a file that exists.
- [ ] 5.3 Verify no command prints private key material, including `open` and the rotation report.

## 6. Documentation

- [ ] 6.1 Rewrite the encryption section of `cli/README.md` around recipients, the agreement key and `open`, and verify every documented command runs as written.
- [ ] 6.2 Rewrite the sealing section of `extension/README.md`, including what a reader sees for somebody else's record and for an algorithm it lacks.
- [ ] 6.3 State the group limitation and the first-contact error in both READMEs, and verify the wording matches what the commands do.

## 7. Integration

- [ ] 7.1 Run the end-to-end scenarios and verify a sealed publish reads as opened for its recipient, as "sealed to" for a third session, and that verdicts are unchanged by any of it.
- [ ] 7.2 Verify the same scenarios pass against both storage engines.

## Workflow follow-up

- Archive the change after the project's review requirements are satisfied.
- Verify the archived result.
