# pi-msgq

A durable message bus for pi and maki sessions, and the agents you build on top of it.

Any process can publish a message to an address — a shell script, a cron job, your own fingers, or
another pi. A session subscribes to its own address and receives it. One SQLite file, no daemon, no
broker, no network.

Sessions find each other on the bus. Start them however you like — another terminal, a script, a
launcher of your own — give each a name, and address it.

```
   you, a script, or another agent
        |
        |  msgq publish session/reviewer "review this diff"
        v
   ~/.pi/agent/msgq/msgq.db      durable topics · presence records · acknowledgements
        ^
        |  the extension subscribes and delivers into the session
        |
   pi session          pi session          maki session
```

| Folder | What it is |
|---|---|
| [`cli/`](cli/README.md) | `msgq`, the bus: publish, read, follow, await, ask, sessions, prune |
| [`extension/`](extension/README.md) | the pi extension: consumes the bus, gives your agent one tool and you three commands |
| [`maki/`](maki/README.md) | the maki plugin: steers a maki session from the shell, with `/peers`, `/peer`, `/reply` |
| [`PROTOCOL.md`](PROTOCOL.md) | what the wire promises, and the fixtures that keep two implementations agreeing |

Two roles, and it helps to keep them apart: the **wire** is what other clients may depend on
(`PROTOCOL.md`), and the **extension** is the reference client. The end-to-end suite is the
showcase: nothing on the wire depends on it.

Building from a checkout, or running the tests? See [DEVELOPMENT.md](DEVELOPMENT.md).

## Install

```bash
cargo install --path cli                              # the bus, as `msgq`
```

The extension is **loaded**, not installed:

```bash
pi -e /path/to/pi-msgq/extension
```

Start a session worth talking to:

```bash
pi -e ./extension --msgq-policy honor --name planner
```

## Several sessions, one question

The shape this was built for: one session hands the same question to several peers with different
instructions, and compares what comes back.

Start each reviewer yourself, wherever you like, with its own name and its own model:

```bash
pi -e ./extension --msgq-policy honor --name harsh --model provider/cheap-model
pi -e ./extension --msgq-policy honor --name rust  --model provider/strong-model
```

Then ask from a third session, or from a shell:

```bash
msgq ask session/harsh '/harsh-review the parser diff' --from session/me
msgq ask session/rust  '/rust-review the parser diff'  --from session/me
```

Each reviewer is a real `pi` with its own context, its own cost and its own transcript. The
comparison is yours to read — the bus carries messages, not verdicts. Starting the processes is the
job of whatever runs your terminals; the bus only carries what they say to each other.

`--msgq-policy honor` is the flag that matters. The default, `display`, shows incoming messages but
never starts a turn: a session in that mode can be read, and cannot be asked to do anything.

## Message a session

A session subscribes to `session/<name>`. Give it a name and the address is predictable:

```bash
msgq publish session/planner "the build is green" --type note
```

```text
msgq note
cli/alice@mac → session/planner

the build is green

reply: /peer cli/alice@mac ...
```

The type of a record says what it is allowed to do, and the session's policy says whether it may:

| `type` | the session |
|---|---|
| `note`, `event` | shows it, and nothing else |
| `prompt` | starts a turn |
| `steer` | redirects the turn already running |
| `follow_up` | queues behind it |

Records that arrive mid-run are **coalesced**: however many arrive during one turn, they are
delivered as a single message and cost at most one more model request.

```bash
msgq sessions      # who is online, and what they are doing
msgq consumers     # which addresses are being followed
msgq read session/planner --from-start    # the retained history
```

## Ask, and get an answer back

`msgq ask` publishes a question and waits. It ends on a reply, on a terminal acknowledgement, or on
its timeout — and a timeout is not an error.

```bash
msgq ask session/planner "which tests should I run?" --from session/me
```

Any record can ask to be told what happened to it, and you can wait for that separately:

```bash
msgq publish session/planner "run the suite" --type prompt --await
msgq await 42 --on cli/ci-bot        # by offset
```

The phases are `received` (responsibility taken, with the delivery mode), `started`, and
`completed` (with an outcome). A display-only delivery stops at `received`: the record was shown,
not acted on.

## Three commands for you

The extension is not only for the agent. From the prompt, no model turn is spent:

```
/peers                          who is reachable; choose one, compose, send
/peer reviewer "take a look"    send to session/reviewer
/reply on it                    answer the message you last received
/peers me                       who you are on the bus: your address, policy, trust
```

A bare name is a session, `me` is this session, and nothing waits for a reply — an answer arrives as
an ordinary message, and the transcript shows how to answer each one. See the
[extension README](extension/README.md#commands-for-you) for the details.

## One tool for your agent

| Tool | Actions |
|---|---|
| `peers` | `list` who is reachable · `send` a message · `ask` a question and wait · `reply` to the message you last received |

A peer is any other participant: another pi session, a maki session, a script, a CI job. The tool
does not start one — that is the job of whatever runs your sessions. It finds them, talks to them,
and waits for the answer:

```text
ask session/reviewer to review the parser, and tell me what it said
```

Under the hood that is `peers({action:"ask",to:"session/reviewer",...})`, then one sentence back to
you.

## The same bus, from maki

A [maki](https://github.com/earendil-works/maki) session can join the same bus with the plugin in
[`maki/`](maki/README.md). It listens on its own `session/<session-id>` and delivers into the
session, and `name` adds a memorable alias; it also gives you the same human commands:

```lua
-- ~/.config/maki/init.lua
require("msgq").setup({ name = "planner" })
```

```bash
msgq publish session/planner "run the tests and fix failures" --ack --await
msgq ask session/planner "what does this repository do?"
```

The maki agent gets a `msgq` tool to send, reply and list peers; `/peers me` reports the address
and bus state. `--with-offset` on `msgq read`/`follow` is what lets it name a record for a reply.

Two maki instances can share `name`: each sends as its own `session/<session-id>`, listens there
as well, and follows with a consumer of `maki#<session-id>`, so a shared alias fans out to all of
them instead of colliding on the follow lock. Address the id for one session, the alias for the
group; answers always come back from the id.

## What it will not do

- **No daemon, no broker, no network.** The bus is one SQLite file, so the reach of the system is the
  reach of the file. Nothing crosses machines.
- **No process management.** The bus carries messages; it does not start, stop or supervise the
  sessions at either end. A peer is a process somebody else started.
- **No handover or context transfer.** You say what you want; you do not send your conversation.
- **No attachments, and no result payloads.** An acknowledgement says what happened, not what was
  produced. The answer is an ordinary message.

## Requirements

| Need | For |
|---|---|
| `pi` (Node 22.19+) | everything |
| a Rust toolchain with edition 2024 (1.85+) | building the CLI |
| `jq`, `glab` | [`scripts/gitlab-watch.sh`](scripts/gitlab-watch.sh) only |
| maki 0.7+ | the [`maki/`](maki/README.md) plugin only |

## Troubleshooting

| Symptom | Likely cause |
|---|---|
| Nothing arrives in a session | the wrong address. `msgq consumers` shows what it follows |
| A message appears but the agent ignores it | the default `display` policy. Start with `--msgq-policy honor` |
| A peer does nothing when messaged | its trust list excludes you, or its policy is `display` |
| `msgq disabled` in the session | the bus could not be opened — check `MSGQ_DB` |
| `msgq exited with 1 and said nothing` | it is not on `PATH`. See [DEVELOPMENT.md](DEVELOPMENT.md) |
