## Context

This is a greenfield repository; no code or specs exist yet. See `proposal.md` for motivation.

Two external facts shape the design:

- **honker 0.5.0** (Rust crate, MIT/Apache-2.0, self-described alpha) provides durable streams, per-consumer offsets, and a cross-process commit watcher over SQLite. Its Rust binding registers all SQL functions in-process (`attach_honker_functions`), so no loadable extension or native artifact is required at runtime.
- **pi's wire vocabulary** already defines exactly the verbs this bus needs: the RPC commands `prompt`, `steer`, `follow_up`, the message type `CustomMessage` for display-only injection, and session events such as `agent_settled`.

Storage facts confirmed from the engine source: `_honker_stream(offset PK AUTOINCREMENT, topic, key, payload TEXT, created_at)` with an index on `(topic, offset)`; `_honker_stream_consumers(name, topic, offset)` with a `(name, topic)` primary key and a monotonic upsert; `payload` is JSON text because the Rust `publish` serializes it; a consumer with no saved offset reads from offset 0 (earliest). The engine sets `busy_timeout=5000` and WAL mode.

## Goals / Non-Goals

**Goals:**

- A durable, topic-addressed bus usable from any local process, with no daemon and no network.
- A record contract that a future pi extension can consume with no translation layer.
- Correct fan-out (independent consumer cursors) and at-most-one-message replay on interruption.

**Non-Goals:**

- The pi extension, injection, steering, or auto-trigger. Designed-for, not built.
- herdr integration.
- Retention/pruning, authentication, multi-host operation, Windows-specific support.
- Exactly-once delivery. The contract is at-least-once with a bounded replay window of one message.

## Decisions

### D1. honker as the storage engine
Use `honker = "=0.5.0"` rather than a hand-rolled SQLite schema.

- Alternatives: a hand-rolled three-table schema on `rusqlite`; a file/Maildir layout as pi-messenger uses; Redis.
- Rationale: the engine already provides the exact primitives (indexed durable streams, monotonic per-consumer offsets, a 1 ms cross-process wake without polling tables) plus future queue and scheduler primitives for the multi-agent case. A hand-rolled schema would be roughly equivalent code with no watcher and no forward path.
- Hedge: all engine use is isolated behind a `store` module. The record contract and address grammar are independent of the engine, so swapping the engine is a module rewrite, not a protocol change.

### D2. Address equals topic equals stream name
A message's `to` field is the canonical address, and that same string is the engine stream name. `from` is the sender's address.

- Alternatives: a fixed topic plus in-record recipient filtering (a shared log); per-recipient inbox topics with a separate routing table.
- Rationale: one routing concept instead of two. Fan-out happens at subscribe time, which the engine's independent cursors already support. Records stay self-describing when exported because `to` travels with them.
- Reserved schemes: `session/`, `cwd/`, `group/`, `cli/`. A bare name canonicalizes to `group/<name>` so one logical group cannot accidentally become two streams.

### D3. The record is pi-native and minimal
`type ∈ {prompt, steer, follow_up, note, event}` maps directly onto pi calls: `prompt` to `sendUserMessage`, `steer`/`follow_up` to `deliverAs` variants, `note` to a display-only `sendMessage`, `event` to a consumed session event. Body fields mirror pi's RPC `prompt` exactly (`message` plus `images`).

- Alternatives: a bespoke vocabulary with an explicit `policy` enum; a `content[]` block array.
- Rationale: the future extension becomes a one-line dispatch instead of a translation layer, and a record is a valid pi RPC command plus envelope metadata.
- Deliberate omissions (each was a footgun, not a feature):
  - **No `ts`.** Two timestamps where one is authoritative invites errors. The engine's `created_at` (unix seconds, set on insert) is the only time source.
  - **No `streamingBehavior`.** Redundant with `type` and able to contradict it.
  - **No `id`.** It implies deduplication the engine does not provide. Correlation is `reply_to`, an engine offset.
- `meta` is kept: explicitly free-form, never read by the bus.

### D4. Opaque payload at the storage layer, blessed convention at the CLI
Shorthand modes build a record; `--json` and stdin pass a record through byte-for-byte. The bus never rewrites a supplied body.

- Alternatives: validate and reject malformed records on publish.
- Rationale: keeps the bus generic and forward-compatible while making pi-shaped records first-class. Consumers tolerate unknown `type` and are expected to ignore fields they do not understand.

### D5. Split stateless `read` from cursor-based `follow`
`read` never touches a cursor; `follow` requires `--consumer` and advances one.

- Alternatives: one verb with a `--consume` flag.
- Rationale: prevents an exploratory read from silently consuming messages, which is the most likely misuse of a bus CLI.

### D6. New consumers start at "now"
Although the engine defaults a missing cursor to offset 0, a consumer's first `follow` starts at the current end unless `--from-start` or `--from N` is given.

- Alternatives: follow the engine default (replay everything).
- Rationale: matches subscription intuition and avoids replaying an entire topic history on first attach. The divergence is deliberate and stated in the spec.

### D7. One global database
Default path `~/.pi/agent/msgq/msgq.db`, resolved as `--db` > `MSGQ_DB` > default. No config file.

- Alternatives: per-project databases under `.pi/`; XDG data dir.
- Rationale: the bus is global by decision, and colocating with pi's other state keeps discovery obvious. No config file keeps scripting trivial and the only persistent state the database itself.

### D8. Owned metadata table
A single `msgq_meta(key, value)` table records `protocol_version` and `created_by`.

- Alternatives: rely on the engine's schema version; store nothing.
- Rationale: gives `db info` a version to report and lets a future reader refuse an unknown protocol. It never touches the engine's `_honker_*` tables on the write path.

### D9. Advisory lock for follow
A follow takes an engine advisory lock named for `(consumer, address)` and fails loudly if held.

- Alternatives: document the hazard only; use SQLite `BEGIN IMMEDIATE` to hold a write lock.
- Rationale: cheap, uses an existing primitive, and converts silent cursor corruption into a clear error.

### D10. Interruption safety
A follow persists the cursor before reporting each message.

- Alternatives: rely on the engine's default batch save of 1000.
- Rationale: the CLI is not a high-throughput worker; one-message granularity makes Ctrl-C essentially lossless at negligible cost.

## Risks / Trade-offs

| Risk | Mitigation |
|---|---|
| The engine is alpha and may change schema or API | Pin `=0.5.0` exactly; keep engine use behind a `store` module; the record contract and address grammar are engine-independent |
| Unbounded growth: the engine has no stream trim | Ship `db info` with per-stream counts and file size; reserve `msgq prune` for a later change; document that pruning must not delete at or above the lowest live cursor |
| `from` is spoofable by any process that can write the database | Document the filesystem as the trust boundary; no authentication in this change; keep the field named `from` and treat it as unverified everywhere |
| Payloads become an execution surface once an agent consumes them | Document that the future extension treats payloads as untrusted, defaults to display-only, and must not auto-`prompt` or auto-`steer` from unverified senders |
| A rename of a pi session changes its `session/<alias>` address, stranding history | Accepted for now; alias-to-UUID resolution belongs with the extension, which knows its own alias |
| WAL and shared-memory sidecar files carry data and permissions | Apply owner-only permissions to the database and its sidecar files on creation |
| `busy_timeout=5000` can block a CLI invocation under heavy contention | Accepted; writes are single-row, and long-lived contention is not expected on a local bus |

## Migration Plan

New installation only; there is no existing data or protocol.

- Deploy with `cargo install --path cli` (source) or a release binary. No service, so no rolling upgrade.
- Rollback is deleting the database file, which discards all messages. Document this.
- Forward compatibility rests on `v` (record protocol) and `msgq_meta.protocol_version` (database protocol). A future reader MUST refuse a higher protocol version rather than guess.

## Open Questions

- **Retention policy.** The shape of `msgq prune` (by age, count, or offset watermark relative to live cursors) is deferred and does not affect this change's specs.
- **Alias-to-UUID resolution.** Whether the CLI should resolve `session/<alias>` to a stable identity is deferred to the extension change, which is where the alias is actually known.
- **Windows.** Path and permission handling are Unix-oriented; Windows support is deferred and would only affect the storage and permission requirements.
- **Multi-host.** The engine is explicitly single-machine and file-backed; distributed operation is out of scope and would require a different transport.
