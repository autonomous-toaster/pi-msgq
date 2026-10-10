## 1. Project scaffold

- [x] 1.1 Create the `cli/` Rust package (package `pi-msgq`, `[[bin]] name = "msgq"`, `edition = "2024"`) with `src/lib.rs` and `src/main.rs`, and verify `cargo build` succeeds from `cli/`.
- [x] 1.2 Add `honker = "=0.5.0"` plus `serde`, `serde_json`, `clap`, and `dirs` to `Cargo.toml`; verify `cargo build` resolves and links.
- [x] 1.3 Add a committed `cli/rust-toolchain.toml`; verify `cargo build` uses it.
- [x] 1.4 Add a root `.gitignore` covering `cli/target/` and the local database path; verify `git status` shows no build artifacts.

## 2. Storage and configuration

- [x] 2.1 Implement database path resolution (`--db` > `MSGQ_DB` > `~/.pi/agent/msgq/msgq.db`) in `lib`; verify with unit tests covering all three precedence cases.
- [x] 2.2 Implement open-or-create using the engine's `Database::open` so first use initializes the schema; verify a command against a temp path creates the file and exits 0.
- [x] 2.3 Add the owned `msgq_meta(key, value)` table and write `protocol_version = 1` and `created_by` on creation; verify `db info` reports the version.
- [x] 2.4 Restrict database and sidecar file permissions to owner-only when the CLI creates the file, and leave pre-existing files untouched; verify with `stat` in a test.

## 3. Address grammar and record model

- [x] 3.1 Implement address parsing and canonicalization (reserved schemes `session/`, `cwd/`, `group/`, `cli/`; bare name becomes `group/<name>`; reject unknown prefix, empty, and scheme-only); verify unit tests for each accept/reject case.
- [x] 3.2 Implement the record type (`v`, `type`, `from`, `to`, `message`, `images`, `reply_to`, `meta`, `event`) with serde, defaulting `from` to `cli/<user>@<host>`; verify a round-trip serialization test.
- [x] 3.3 Implement passthrough storage that preserves unknown fields when a full record is supplied; verify a test publishes a record with an extra field and reads it back unchanged.
- [x] 3.4 Reject a record that omits `v`, `type`, `from`, or `to`, and require `event` when `type` is `event`; verify failure cases return a usage error.

## 4. Publish

- [x] 4.1 Implement `msgq publish <address> [message]` appending one message and printing the assigned offset; verify the offset increases across two publishes.
- [x] 4.2 Support `--from`, `--type`, `--json`, and `-` (body or record from stdin); verify each mode with an integration test.

## 5. Stateless read

- [x] 5.1 Implement `msgq read <address>...` with `--from-start`, `--from N`, and default "current end"; verify a test that reads twice without consuming.
- [x] 5.2 Implement multi-address reads merged by offset across independent streams; verify ordering in a test with two streams.

## 6. Cursor-based follow

- [x] 6.1 Implement `msgq follow <address>... --consumer <name>` resuming from the saved offset, starting at the current end for a new consumer; verify a new consumer does not replay history.
- [x] 6.2 Persist the cursor before reporting each message; verify by killing a follow and confirming at most one message replays.
- [x] 6.3 Handle SIGINT by saving the cursor and exiting 0; verify with an integration test that sends SIGINT.
- [x] 6.4 Take an advisory lock per consumer/address and fail loudly when it is already held; verify a second concurrent follow exits non-zero.

## 7. Topics, consumers, and cursors

- [x] 7.1 Implement `msgq topics` listing distinct canonical addresses with message count and highest offset; verify against a seeded database.
- [x] 7.2 Implement `msgq consumers [--topic T]` and `msgq consumers rm <name> [--topic T]`; verify removal removes only matching rows.
- [x] 7.3 Implement `msgq cursor get|set|reset`; verify that setting a lower offset does not decrease the stored value.

## 8. Database info and output conventions

- [x] 8.1 Implement `msgq db info` reporting path, existence, size, per-stream counts, consumer counts, and protocol version; verify output contains each field.
- [x] 8.2 Implement `--json` emitting raw stored records as JSON lines for read and follow; verify each line parses as JSON.
- [x] 8.3 Implement exit codes 0/1/2 for success, runtime error, and usage error; verify an unknown flag exits 2.

## 9. Verification

- [x] 9.1 Add integration tests using a temporary database exercising publish, read, follow, topics, consumers, and cursor; verify `cargo test` passes from `cli/`.
- [x] 9.2 Run an end-to-end acceptance check with two shells: publish from one and follow from the other, confirming cross-process delivery and cursor persistence across restart.
- [x] 9.3 Confirm the extension-facing dispatch table in `design.md` (D3) matches the emitted record shapes by publishing one record of each `type` and inspecting the stored JSON.
