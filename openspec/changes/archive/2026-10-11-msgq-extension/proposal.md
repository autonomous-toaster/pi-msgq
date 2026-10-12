## Why

The `msgq` CLI can publish durable, topic-addressed messages, but nothing inside a pi session
consumes them. A running session is otherwise unreachable from scripts and operators: pi's RPC
mode requires owning the process from launch, and the existing ecosystem extensions that do
cross-session messaging are agent-facing and use their own transports. This change adds the
consuming half — a pi extension that subscribes to the bus and delivers messages into the
conversation. It is the CLI-to-session half of the eventual multi-agent story; agent-to-agent
coordination remains out of scope.

## What Changes

- Add a pi extension under `extension/`, loaded ad hoc with `pi -e ./extension` (not installed,
  not published).
- Consume the same SQLite bus as the CLI through `@russellthehippo/honker-node`: durable streams,
  monotonic per-consumer offsets, and a shared cross-process commit watcher. No separate broker
  or daemon, and no polling timer.
- Subscribe by default to the session's own address `session/<self>`, with optional additional
  topics (`cwd/<enc>`, `group/<name>`) selected by configuration.
- Gate delivery by a subscriber policy that caps publisher intent: `display` (default) delivers
  every record as a display-only note and never starts a turn; `honor` additionally lets
  `prompt`, `steer`, and `follow_up` records act on the session.
- Coalesce turn-worthy records that arrive while the agent is running into a single injected
  message and a single continuation at `agent_before_settle`.
- Persist the consumer cursor before reporting each record, so an interruption replays at most
  one message.
- Stop cleanly on session shutdown: close subscriptions and the engine handle.

Explicitly not in this change:

- No publishing path: the extension is read-only. Senders use the `msgq` CLI.
- No agent-to-agent messaging, replies, or handshakes.
- No presence or discovery publishing; naming a session is the sender's concern.
- No distribution via `pi install`, npm, or a package manifest.
- No changes to the CLI or its record contract.

## Capabilities

### New Capabilities

- `msgq-extension`: a pi extension that consumes msgq topics and delivers records into the
  session according to a subscriber policy, with durable per-consumer progress and coalesced
  turn delivery.

### Modified Capabilities

<!-- None. The CLI and its record contract are unchanged. -->

## Impact

- New TypeScript extension at `extension/` (`index.ts`, `package.json`), loaded with
  `pi -e ./extension`.
- New runtime dependency `@russellthehippo/honker-node` (native, prebuilt for macOS and Linux),
  expected to stay aligned with the CLI's honker engine generation.
- Consumes the existing bus at `~/.pi/agent/msgq/msgq.db` or `MSGQ_DB`; no CLI change and no new
  bus schema.
- Gives runtime meaning to the record `type` values already defined by `msgq-cli` and introduces
  the subscriber policy vocabulary (`display`, `honor`).
