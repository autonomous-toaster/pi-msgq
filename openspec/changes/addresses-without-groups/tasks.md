# Tasks

## 1. The grammar

- [ ] 1.1 Replace the reserved schemes with `session`, `cli` and `bus`, canonicalize a bare name to `session/<name>`, and verify a test asserts each of those, plus a usage error naming the schemes that exist for `group/…` and `cwd/…`.
- [ ] 1.2 Update every call site and example that built a `group/` or `cwd/` address, including the queue-name slug in `store.rs`, and verify `cargo build` and a search for the two removed schemes find nothing.

## 2. The roster's home

- [ ] 2.1 Move presence to `bus/presence` and `bus/<scope>/presence`, and verify `msgq sessions` and the roster round-trip against both.
- [ ] 2.2 Assert a presence record is never sealed, whatever key material is configured, and verify the test fails if sealing is re-enabled for `event` records with `kind: presence`.
- [ ] 2.3 Point the extension at the new roster address and verify `/peers`, `msgq sessions` and the locator card all still work end to end on both engines.

## 3. Fixtures and tests

- [ ] 3.1 Rewrite the fixtures that name `group/…` or `cwd/…` against the three remaining schemes, keeping the same cases, and verify both clients still accept and reject the same set.
- [ ] 3.2 Rename the convenient `group/a` address used across the test suites to `session/a`, and verify the suites pass without a single `group/` left in them.

## 4. Documentation

- [ ] 4.1 Rewrite the address table and its examples in `cli/README.md`, the examples in `README.md`, and the presence section of `extension/README.md`, and verify every documented command runs as written.
- [ ] 4.2 State in `PROTOCOL.md` that the roster address is public and unsealed, and verify the conformance fixtures agree.

## Workflow follow-up

- Archive the change after the project's review requirements are satisfied.
- Verify the archived result.
