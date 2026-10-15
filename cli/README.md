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
| `type` | yes | one of `prompt`, `steer`, `follow_up`, `note`, `command`, `event` |
| `from` | yes | sender address (defaults to `cli/<user>@<host>`) |
| `to` | yes | recipient address; must equal the stream it is published to |
| `message` | for message types | the body |
| `images` | no | pi `ImageContent` blocks |
| `event` | for `type: "event"` | a pi session event, instead of `message` |
| `reply_to` | no | the offset of the message this one answers; see [Record ids](#record-ids) |
| `meta` | no | free-form; never read by the bus |

There is deliberately no publisher-supplied timestamp. The bus stamps `created_at` on insert
and that is the only time source.

### Record ids

Every record has an id: `<address>#<offset>`, the stream that holds it and the offset it was
assigned there. Both halves are already in the record and in what `publish` prints, so an id is
derived rather than stored — it is valid for every record ever published, including ones written
before you read this.

```
session/api-worker#42
  \_____________/  \/
   the stream      the offset in it
```

`reply_to` names the record a message is about. It is an offset in the stream named by *that*
record's own `from`, so the record a reference names is always `<from>#<reply_to>`. That is what
makes a reference exact rather than a guess, and it is why a reply is published by the address the
original was addressed to: only that address received it.

Offsets are global to the database, not per stream, so one stream's offsets are sparse. An id is
still the pair, because the offset alone does not say which stream to look in.

### How `type` maps onto pi

| record `type` | intended pi call |
|---|---|
| `prompt` | `sendUserMessage(message)` — starts or queues a turn |
| `steer` | `sendUserMessage(message, { deliverAs: "steer" })` — mid-run, at the next model boundary |
| `follow_up` | `sendUserMessage(message, { deliverAs: "followUp" })` — after the current run |
| `command` | `sendUserMessage(message, { expandPromptTemplates: true })` — the session runs it as if typed |
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
| `sessions` | list live participants holding a bus lease |
| `await <OFFSET>` | wait for acknowledgements of a published message |
| `ask <ADDRESS> [MESSAGE]` | ask a peer a question and wait for the answer |
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

msgq publish <address> "an answer" --reply-to 7                     # by offset
msgq publish <address> "an answer" --reply-to session/api-worker#7   # or by id
msgq publish <address> "/harsh-review the parser" --type command    # for the peer to run
msgq publish <address> "a question" --print-id    # print the id, not the offset
```

The bus never rewrites a record supplied with `--json`; unknown fields are preserved. A
record's `to` must match the address argument. `--type event` requires `--json`.

A bare offset is still what `publish` prints by default, so scripts that parse it are unaffected;
`--print-id` is opt-in. An id given to `--reply-to` must name the record's own sender, since a
reference points into that sender's stream — answering something that was addressed to somebody
else is a usage error, and nothing is written.

`--type command` publishes a **command record**, whose `message` is a command line rather than
something to read: the receiving session runs it as if it had been typed. The command must be the
whole body, because that is what a command is, so anything the sender wants known about itself has
to travel outside the body. The bus does not know which commands exist and does not check: an
unknown one is still a valid record, and a consumer that does not know the type shows it like any
other.

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

## Presence

Live sessions advertise themselves with a short **lease** in the bus: renewed while the session
runs, released when it stops. `msgq sessions` lists them.

```text
$ msgq sessions
session/api-worker  session  busy  api-worker  27
session/01a12cc5    session  idle             30
```

Columns are address, kind, state, and remaining lifetime in seconds. `--json` emits one object
per participant.

A session that crashes simply stops renewing, so its lease expires on its own — there is nothing
to clean up and no daemon involved. Presence is **advisory**: it tells you who is reachable right
now, but publishing to an offline session still queues durably. Leases come from the pi
extension, so `msgq sessions` is empty unless at least one session has it loaded.

## Acknowledgements

A sender can ask to be told what happened to a message. `--ack` adds the request for you, and
`meta.ack` does it for a full record:

```bash
msgq publish session/api-worker "Deploy now" --ack                 # every phase
msgq publish session/api-worker "Deploy now" --ack started,completed
```

With a hand-written record:

```bash
msgq publish session/api-worker --json '{"v":1,"type":"prompt","from":"cli/ci-bot",\
  "to":"session/api-worker","message":"Deploy now","meta":{"ack":true}}'
```

The session reports up to three phases, each published back to the sender's address:

| Phase | Meaning |
|---|---|
| `received` | responsibility taken, and **how** it was delivered |
| `started` | a run that includes it began |
| `completed` | that run settled, with an outcome |

`received` carries a delivery mode — `turn`, `queued` (held while the session was busy), or
`display` (shown only, **terminal**: no run will follow). `completed` carries an outcome:
`ok`, `error`, or `aborted`.

Wait for them with `--await`, or re-attach with `await`:

```text
$ msgq publish session/api-worker "Deploy now" --await
7
received delivery=turn
started
completed outcome=ok
```

`--timeout <seconds>` bounds the wait (default 300, `0` for no limit). A wait that ends without a
terminal acknowledgement exits with status **3**, so a script can tell "timed out" from "errored".
`await` finds the acknowledgements by asking whoever sent the referenced message, so it works
regardless of which address the replies landed on. `--on <address>` overrides that when you need
to point it somewhere else.

Note where replies go: acknowledgements are addressed to the **sender**, not to the stream the
message was published to. `msgq follow session/<id>` shows messages sent *to* a session; follow
your own address (`msgq follow cli/<user>@<host>`) to watch the lifecycle.

Acknowledgements are ordinary `event` records carrying `reply_to`, so `msgq read <your-address>`
shows them and any third party following both sides can watch the exchange. Human-readable output
renders them as a lifecycle line rather than raw JSON:

```text
$ msgq follow cli/ci-bot
      9  2026-10-11T22:04:00Z  cli/ci-bot      ack   session/child   #7 received turn
     10  2026-10-11T22:04:01Z  cli/ci-bot      ack   session/child   #7 started
     11  2026-10-11T22:04:09Z  cli/ci-bot      ack   session/child   #7 completed ok
```

`#7` is the offset of the message being acknowledged.

## Asking a peer

`msgq ask` publishes a question that asks for an answer, then waits for one:

```bash
msgq ask session/reviewer "Review the diff on this branch and tell me what you find"
msgq ask session/reviewer "and the tests?" --from session/planner
```

It ends in one of three ways, and **none of them is an error**: the reply is printed as the
command's output; or the peer finishes without answering, which is reported; or the wait window
closes first, which is reported as "still running" (the peer is fine, it is just slow).
`--timeout <seconds>` defaults to 600, and `0` waits without a limit.

Replies are ordinary records carrying `reply_to`, so `msgq publish --reply-to <offset>` will craft
one by hand, and `--reply-to <address>#<offset>` names one exactly. In practice you rarely do: a
peer that is asked a question is told, in the message it receives, to answer with the `peers` tool.

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
