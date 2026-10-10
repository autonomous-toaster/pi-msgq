## Why

pi sessions are otherwise isolated: an external script or human has no durable way to deliver a message to a running or future session. Existing tools in the pi ecosystem (pi-intercom, pi-messenger) solve the live, agent-facing case with a broker or shared files, but none provides a durable, inspectable, topic-addressed bus that any process can publish to without owning the pi process.

This change builds that bus as a standalone CLI backed by honker's SQLite stream primitives. It is the transport layer for a future multi-agent ecosystem; the pi extension that consumes from it is explicitly out of scope here.

## What Changes

- Add a Rust CLI, `msgq`, that publishes and reads durable topic messages in a single global SQLite database.
- Address model: **address == topic == honker stream name**. Reserved schemes `session/<id>`, `cwd/<enc>`, `group/<name>`, `cli/<who>`; a bare name is canonicalized to `group/<name>`.
- Define a pi-native message record: `{v, type, from, to, message, images?, event?, reply_to?, meta?}` where `type ∈ {prompt, steer, follow_up, note, event}` maps directly onto pi's RPC/message vocabulary.
- Provide publish, stateless read, cursor-based follow, topic listing, consumer/cursor management, and database info.
- Persist consumer cursors via honker's monotonic per-consumer offsets; fanout is native (independent cursors per consumer).
- Add an owned `msgq_meta` table recording `protocol_version` and provenance; never modify honker's `_honker_*` tables on the write path.

Non-goals for this change:

- No pi extension, injection, steering, or auto-trigger. The extension is designed-for, not built.
- No herdr integration.
- No retention/pruning (`msgq prune` is reserved for a later change), no authentication, no remote/network transport.

## Capabilities

### New Capabilities

- `msgq-cli`: the `msgq` command-line message bus — storage location and resolution, address grammar and canonicalization, the message record contract, and the publish/read/follow/topics/consumers/cursor/db operations.

### Modified Capabilities

<!-- None. This is the first capability in this repository. -->

## Impact

- New Rust crate under `cli/` (package `pi-msgq`, binary `msgq`). Repository root stays clean.
- New runtime dependency on `honker = "=0.5.0"` (MIT/Apache-2.0), pinned exactly while it is alpha.
- Introduces a global database at `~/.pi/agent/msgq/msgq.db` (overridable by `MSGQ_DB` then `--db`).
- No existing code, APIs, or specs are modified.
- Defines the protocol contract a future pi extension will consume; changing the record shape after this ships is a breaking change.
