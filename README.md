# pi-msgq

A durable message bus for pi, and the **companion agents** you build on top of it.

Any process can publish a message to an address — a shell script, a cron job, your own fingers, or
another pi. A session subscribes to its own address and receives it. One SQLite file, no daemon, no
broker, no network.

Two pi sessions can also *start* each other: a session spawns a companion in a terminal tab, hands it
work as a message, and gets the answer back.

```
   you, a script, or another agent
        |
        |  msgq publish session/reviewer "review this diff"
        v
   ~/.pi/agent/msgq/msgq.db      durable topics · presence leases · acknowledgements
        ^
        |  the extension subscribes and delivers into the session
        |
   pi session  --(spawn)-->  a herdr tab running its own pi   (a "companion")
```

| Folder | What it is |
|---|---|
| [`cli/`](cli/README.md) | `msgq`, the bus: publish, read, follow, await, ask, sessions |
| [`extension/`](extension/README.md) | the pi extension: consumes the bus, gives your agent two tools and you four commands |
| [`companion/`](companion/README.md) | `companion`: starts and stops companions in herdr tabs |

Building from a checkout, or running the tests? See [DEVELOPMENT.md](DEVELOPMENT.md).

## Install

```bash
cargo install --path cli                              # the bus, as `msgq`
ln -s "$PWD/companion/companion" ~/.local/bin/companion
```

The extension is **loaded**, not installed:

```bash
pi -e /path/to/pi-msgq/extension
```

Start a session worth talking to:

```bash
pi -e ./extension --msgq-policy honor --name planner
```

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

## Four commands for you

The extension is not only for the agent. From the prompt, no model turn is spent:

```
/peers                          who is reachable; choose one, compose, send
/peer reviewer "take a look"    send to session/reviewer
/reply on it                    answer the message you last received
/peers me                       who you are on the bus: your address, policy, trust
/companions                     what is running in terminal tabs, and where
```

A bare name is a session, `me` is this session, and nothing waits for a reply — an answer arrives as
an ordinary message, and the transcript shows how to answer each one. See the
[extension README](extension/README.md#commands-for-you) for the details.

## Two tools for your agent

| Tool | Actions |
|---|---|
| `peers` | `list` who is reachable · `send` a message · `ask` a question and wait · `reply` to the message you last received |
| `companions` | `spawn` a companion (a real pi in its own tab, reachable at `session/<name>`) · `close` it |

Spawning creates a process and nothing else — the task is simply the first message:

```text
start a companion called reviewer, ask it to review the parser, and tell me what it said
```

Under the hood that is `companions({action:"spawn",name:"reviewer"})`, then
`peers({action:"ask",to:"session/reviewer",...})`, then one sentence back to you.

## What it will not do

- **No daemon, no broker, no network.** The bus is one SQLite file, so the reach of the system is the
  reach of the file. Nothing crosses machines.
- **No in-process subagents.** A companion is a real `pi` process in its own tab: it has its own
  context, its own cost, and its own transcript you can go and read.
- **No handover or context transfer.** You say what you want; you do not send your conversation.
- **No attachments, and no result payloads.** An acknowledgement says what happened, not what was
  produced. The answer is an ordinary message.
- **No orphan reaping.** A companion that never finishes keeps its tab, on purpose, so you can look
  at what went wrong.

## Requirements

| Need | For |
|---|---|
| `pi` (Node 22.19+) | everything |
| a Rust toolchain with edition 2024 (1.85+) | building the CLI |
| `herdr` 0.8+ and `jq`, and running inside herdr | companions only |

Three of the four commands — everything but `/companions` — and both of the agent's tools work
without herdr. Only listing and starting a companion need it.

## Troubleshooting

| Symptom | Likely cause |
|---|---|
| Nothing arrives in a session | the wrong address. `msgq consumers` shows what it follows |
| A message appears but the agent ignores it | the default `display` policy. Start with `--msgq-policy honor` |
| A companion does nothing when messaged | it was spawned without `--parent`, so it trusts nobody |
| A companion never becomes reachable | the session has presence off; `spawn` gates on presence |
| `msgq disabled` in the session | the bus could not be opened — check `MSGQ_DB` |
| `companion exited with 1 and said nothing` | it is not on `PATH`. See [DEVELOPMENT.md](DEVELOPMENT.md) |
