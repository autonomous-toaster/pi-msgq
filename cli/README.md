# msgq

Durable, topic-addressed message bus for pi — one SQLite file, no daemon, no network.

Any local process (a shell script, a cron job, another agent) can publish a message to an
address; anything that can read the file can follow it. `msgq` is the transport layer for a
future multi-agent pi ecosystem. The pi extension that consumes from it is designed-for but
not built here.

```
   publisher (script, human, agent)                 subscriber (script, agent, you)
   -------------------------------                 --------------------------------
   msgq publish session/api-worker "build is green"
                    |
                    v
            +-------------------------------+
            |  ~/.pi/agent/msgq/msgq.db     |   durable stream + per-consumer cursor
            |  (SQLite, WAL)                |
            +-------------------------------+
                    |
                    v
                                    msgq follow session/api-worker --consumer me
```

## Status

Early. The CLI and its protocol are implemented and tested; the pi extension, retention, and
alias resolution are deliberately deferred (see [Non-goals](#non-goals)).

## Install

```bash
cargo install --path cli          # installs the `msgq` binary
# or, from inside cli/
cargo build --release             # ./target/release/msgq
```

Requires a Rust toolchain with edition 2024 support (Rust 1.85+; the repo pins `cli/rust-toolchain.toml`).

## Quick start

```bash
# Publish to a group (bare names are shorthand for group/<name>)
msgq publish build.done "tests are green"          # -> prints the assigned offset

# Read without consuming anything
msgq read build.done --from-start

# Follow as a durable subscription (blocks, resumes where it left off)
msgq follow build.done --consumer ci-watcher

# From another terminal, in the meantime:
msgq publish build.done "deploy finished"
```

```text
$ msgq read build.done --from-start
      1  2026-10-10T21:27:40Z  group/build.done          prompt     cli/you@host  tests are green
      2  2026-10-10T21:27:45Z  group/build.done          prompt     cli/you@host  deploy finished
```

## Concepts

### Addresses

An address is the recipient **and** the stream it is written to. There is exactly one routing
concept; a message's `to` field always equals the stream name.

```
  scheme            example                       meaning
  ----------------  ----------------------------  -------------------------------
  session/<id>      session/api-worker            one pi session's inbox
  cwd/<enc>         cwd/Users-me-proj             every session in that working dir
  group/<name>      group/backend                 a named group anyone may join
  cli/<who>         cli/alice@mac                 a non-pi producer
  <bare-name>       build.done                    shorthand for group/build.done
```

If an address contains `/`, its prefix must be one of the reserved schemes. Addresses are
case-sensitive. Empty addresses and scheme-only addresses (`session/`) are rejected.

`cwd/<enc>` uses pi's session-directory encoding: strip the leading `/` and replace `/`, `\`,
and `:` with `-`.

### The record

Every published message is a JSON object shaped like pi's own wire vocabulary, so a pi
extension can dispatch it with no translation.

```json
{
  "v": 1,
  "type": "prompt",
  "from": "cli/alice@mac",
  "to": "session/api-worker",

  "message": "deploy finished",
  "images": [{ "type": "image", "data": "...", "mimeType": "image/png" }],

  "reply_to": 42,
  "meta": {}
}
```

| Field | Required | Notes |
|---|---|---|
| `v` | yes | protocol version, currently `1` |
| `type` | yes | one of `prompt`, `steer`, `follow_up`, `note`, `event` |
| `from` | yes | sender address (defaults to `cli/<user>@<host>`) |
| `to` | yes | recipient address; must equal the stream it is published to |
| `message` | for message types | the body |
| `images` | no | pi `ImageContent` blocks |
| `event` | for `type: "event"` | a pi session event, instead of `message` |
| `reply_to` | no | a stream offset, for threading |
| `meta` | no | free-form; never read by the bus |

There is deliberately no publisher-supplied timestamp. The bus stamps `created_at` on insert
and that is the only time source.

### How `type` maps onto pi

| record `type` | intended pi call |
|---|---|
| `prompt` | `sendUserMessage(message)` — starts or queues a turn |
| `steer` | `sendUserMessage(message, { deliverAs: "steer" })` — mid-run, at the next model boundary |
| `follow_up` | `sendUserMessage(message, { deliverAs: "followUp" })` — after the current run |
| `note` | `sendMessage({ customType, content, display: true })` — display only, no turn |
| `event` | consumed by extensions; not injected into the conversation |

The publisher only *requests* a behavior; a consuming extension decides whether to honor it.
Keep display-only as the safe default for unverified senders.

## Commands

```
msgq [--db <PATH>] <COMMAND>
```

| Command | Purpose |
|---|---|
| `publish <ADDRESS> [MESSAGE]` | append one message; prints the assigned offset |
| `read <ADDRESS>...` | stateless read; never creates or advances a cursor |
| `follow <ADDRESS>... --consumer <NAME>` | durable subscription; advances the consumer's cursors |
| `topics` | list addresses with message count and highest offset |
| `consumers [--topic <T>]` | list consumer cursors |
| `consumers rm <NAME> [--topic <T>]` | remove consumer cursors |
| `cursor get <CONSUMER> <TOPIC>` | print a saved offset, or `0` |
| `cursor set <CONSUMER> <TOPIC> <OFFSET>` | save an offset (monotonic) |
| `cursor reset <CONSUMER> <TOPIC>` | clear an offset |
| `db info` | path, size, contents, protocol version |

### `publish`

```bash
msgq publish <address> "text"              # type=prompt
msgq publish <address> "text" --type note  # or steer, follow_up
msgq publish <address> "text" --from cli/ci@host
msgq publish <address> -                   # body from stdin
msgq publish <address> --json '<record>'   # a full JSON record
msgq publish <address> --json -            # record from stdin
```

The bus never rewrites a record supplied with `--json`; unknown fields are preserved. A
record's `to` must match the address argument. `--type event` requires `--json`.

### `read`

```bash
msgq read <address>... [--from <N> | --from-start] [--limit <N>] [--json]
```

- Default start position is the **current end** of each stream — reads do not replay history.
- Reading never consumes: run it twice and you get the same messages.
- Multiple addresses are merged in offset order.
- `--limit` defaults to `1000`.

### `follow`

```bash
msgq follow <address>... --consumer <NAME> [--from <N> | --from-start] [--json]
```

- Resumes from the consumer's saved offset.
- A consumer with **no saved offset starts at the current end**, so a new subscription does
  not replay history. Use `--from-start` to replay everything.
- The cursor is persisted **before** each message is reported, so an interruption replays at
  most one message.
- `Ctrl-C` saves the cursor and exits `0`.
- An advisory lock is held per `(consumer, address)`; a second concurrent follower of the same
  pair fails loudly instead of racing the cursor. Locks are renewed while running and expire
  within a minute if the process dies uncleanly.

Fan-out is native: every consumer has its own independent cursor, so publishing once to an
address delivers to every follower.

### `--json`

`--json` is an **input** format for `publish` and an **output** format for `read`, `follow`,
`topics`, and `consumers`. As output it emits the raw stored record, one JSON object per line,
so it pipes straight into `jq`.

## Exit codes

| Code | Meaning |
|---|---|
| `0` | success |
| `1` | runtime or database error |
| `2` | usage error (bad flag, bad address, invalid record) |

## Storage

The database path is resolved in this order:

1. `--db <PATH>`
2. `MSGQ_DB`
3. `~/.pi/agent/msgq/msgq.db`

The file is created and initialized on first use. When `msgq` creates it, it is restricted to
owner-only permissions (`0600`); a pre-existing file is left untouched.

The bus is built on [honker](https://github.com/russellromney/honker)'s durable streams: an
indexed append-only table plus a per-consumer offset table. `msgq` adds one table of its own,
`msgq_meta`, recording the bus protocol version. Engine use is confined to one module so the
record contract and address grammar stay independent of it.

## Non-goals

- **No pi extension.** Injection, steering, and auto-trigger are designed-for, not implemented.
- **No retention.** There is no `prune` yet; streams grow without bound. `db info` reports size
  so growth is visible.
- **No alias resolution.** `session/<x>` is a stream name. Resolving an alias to a stable
  session identity belongs with the extension, which knows its own alias.
- **No authentication.** Any process that can write the database can publish, and `from` is
  unverified. The filesystem is the trust boundary.
- **No network.** Single-machine, file-backed, like the engine.

## Development

```bash
cd cli
cargo test                       # unit + integration tests
cargo clippy --all-targets -- -D warnings
cargo fmt --check
```

Integration tests spawn the built binary against a temporary database, covering publish,
read, follow, topics, consumers, cursors, and usage errors.
