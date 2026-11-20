# Tasks

## 1. The party rule

- [ ] 1.1 Make a publish to a party whose agreement key is unknown fail, naming the address, and verify a test asserts nothing is stored and the exit status is a usage error.
- [ ] 1.2 Delete `--plaintext` and verify it is rejected as an unknown flag, with a test.
- [ ] 1.3 Keep the roster unsealed by rule and verify a presence publish under any configuration stores an unsealed record.

## 2. Deleting the passphrase scheme

- [ ] 2.0 Make the behavioural fixture harness write through the store instead of the CLI, and verify both engines still run every case. Deleting `--plaintext` removes the last way to store a record that is not sealed to a party, and those fixtures publish arbitrary bodies to arbitrary addresses: they are the client with no key, so they have to write bytes rather than ask a client to compose them.

- [ ] 2.1 Delete the scrypt key, `MSGQ_PASSPHRASE`, `MSGQ_PASSPHRASE_FILE`, `MSGQ_KEY_SALT` and the salt-derived key identifier, and verify a test asserts the variables have no effect on what is stored.
- [ ] 2.2 Delete the additional-data version that existed for its records and keep only the binding that names the sender and address, and verify a body moved to another address still fails to open while a current record opens.
- [ ] 2.3 Delete `extension/src/seal.ts` and the last of its callers, and verify the extension's tests still cover opening through `msgq open`.
- [ ] 2.4 Remove the fixtures that exercise the removed scheme and their expectations, and verify the fixture count assertion drops by exactly that number.

## 3. Reporting

- [ ] 3.1 Make `db info` report the one scheme and the agreement key in force, and verify it names no bus key and prints no key material.

## 4. Documentation

- [ ] 4.1 Rewrite the sealing section of `README.md` and `cli/README.md` around one scheme, and verify every documented command runs as written — including that a publish to an unknown party fails as documented.
- [ ] 4.2 Remove the key-identifier paragraph from `PROTOCOL.md` and state the single binding, and verify the conformance fixtures agree.

## Workflow follow-up

- Archive the change after the project's review requirements are satisfied.
- Verify the archived result.
