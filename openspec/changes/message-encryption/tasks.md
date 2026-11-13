# Tasks

## 1. Key custody

- [ ] 1.1 Implement key custody. Derive a key from a passphrase with a slow key-derivation function. Derive the key identifier from the key. Refuse to seal when no passphrase is available and name the absence. Store no key material beside the bus file. Verify a derived key opens a sealed body and that the bus file holds no key material.

## 2. Record layer

- [ ] 2.1 Seal a body on publish in both clients. Use an authenticated cipher with a fresh nonce per record. Write the algorithm and the key identifier into the record metadata. Keep the envelope readable. Verify two seals of one body differ.
- [ ] 2.2 Open a body on read in both clients. Report an error naming the key identifier when the body does not open. Treat no byte of an unopenable body as content. Verify an altered body is refused.
- [ ] 2.3 Keep a mixed history readable in both clients. Read a plaintext record without a key. Read a sealed record with the key. Preserve a sealed record that the reader cannot open. Verify one stream holding both kinds reads whole.

## 3. CLI surface

- [ ] 3.1 Seal on publish in the CLI. Seal when a key is configured. Store a plaintext body when no key is configured. Report the plaintext state. Verify a sealed body is stored for a configured key.
- [ ] 3.2 Open on read in the CLI. Print the content when the body opens. Exit non-zero when the body does not open. Print no body on a failure. Name the key identifier on a failure. Verify an unopenable body exits non-zero and prints no body.
- [ ] 3.3 Report the key in `db info`. Name the key state. Name the key identifier. Print no key material. Verify the report names the identifier and leaks nothing.

## 4. Extension delivery

- [ ] 4.1 Refuse an unopenable body in delivery. Report the error. Start no turn. Publish no acknowledgement. Keep the session running. Name the key state in the identity report. Verify all three consequences hold together.

## 5. Conformance

- [ ] 5.1 Add the fixtures. Add a sealed body. Add a tampered body. Add one stream holding a plaintext body and a sealed body. Verify both clients agree on every encoded case.
- [ ] 5.2 Prove the stored bytes carry no plaintext. Search the bus file for a known body. Search the engine tables for a known body. Verify no match in either.
