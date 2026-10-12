# pi-msgq

A durable, topic-addressed message bus for pi, plus **companion agents**: real `pi` processes you
start in herdr tabs and talk to.

## What is here

| Folder | What it is | Language |
|---|---|---|
| `cli/` | `msgq`, the bus: publish, read, follow, await, ask, sessions | Rust |
| `extension/` | The pi extension: consumes the bus, and exposes the `peers` and `companions` tools | TypeScript |
| `companion/` | `companion`, which starts and stops companions in herdr tabs | shell |

```
  you / a script / an agent
        |
        |  msgq publish session/reviewer "review this"
        v
  ~/.pi/agent/msgq/msgq.db  <-- durable topics, presence leases, acknowledgements
        ^
        |  the extension subscribes and delivers into the session
        |
  pi session  --(companions tool)-->  herdr tab  -->  a companion pi
```

There is no daemon and no broker. The bus is one SQLite file.

## Prerequisites


| Need | For |
|---|---|
| Rust (edition 2024, 1.85+) | the CLI |
| Node 22+ | the extension |
| `pi` on `PATH` | everything |
| `herdr` 0.8+ and `jq` | companions only |
| running inside herdr (`HERDR_ENV=1`) | spawning companions |

## Build

```bash
cargo build --release --manifest-path cli/Cargo.toml
(cd extension && npm install)
```

Nothing installs itself. In a checkout, `msgq` and `companion` are **not on `PATH`**, and that is
the first thing to get right.

## Run it in development

Three variables carry everything, and the extension's tools use them to find their helpers:

```bash
export MSGQ_BIN="$PWD/cli/target/release/msgq"        # the wire
export MSGQ_COMPANION="$PWD/companion/companion"     # the herdr glue
export MSGQ_SPAWN_EXTRA="-e $PWD/extension"          # what a companion should load
```

Then start a session that will actually act on messages:

```bash
pi -e ./extension --msgq-policy honor --name planner
```

`--msgq-policy honor` matters: the default, `display`, shows incoming messages but never starts a
turn, so a companion would never do anything.

Ask it for something that needs a second agent:

```text
start a companion called reviewer and ask it what this repository does
```

Under the hood that is `companions({action:"spawn",name:"reviewer"})` followed by
`peers({action:"ask",to:"session/reviewer",...})`.

### Or do it by hand, without an agent

```bash
./companion/companion spawn reviewer --parent session/planner --json
./cli/target/release/msgq ask session/reviewer "what does this repository do?" --from session/planner
./companion/companion close reviewer
```

`--parent` is both where the companion reports and the **only sender it will ever act on**. A
companion answers its parent and nobody else.

### Install them instead

```bash
cargo install --path cli        # puts msgq in ~/.cargo/bin
ln -s "$PWD/companion/companion" ~/.local/bin/companion
```

With both on `PATH` you can drop `MSGQ_BIN` and `MSGQ_COMPANION`, and set only
`MSGQ_SPAWN_EXTRA` while the extension itself is still loaded with `-e`.

## Tests

```bash
(cd cli && cargo test)                    # 23 unit + 15 integration
(cd extension && node --test test/)       # 75 unit
```

The end-to-end driver drives real `pi` sessions in RPC mode and needs `herdr` for the companion
scenarios:

```bash
export MSGQ_BIN="$PWD/cli/target/release/msgq"
export MSGQ_COMPANION="$PWD/companion/companion"
export MSGQ_SPAWN_EXTRA="-e $PWD/extension"
node extension/scripts/e2e.mjs <scenario>
node extension/scripts/e2e.mjs delegate      # an agent spawns a peer and asks it a question
```

Scenarios cover delivery policy (`default-note`, `honor-prompt`, `note-busy`, `coalesce`),
presence (`presence`, `presence-off`, `rename`, `squat`, `crash-expiry`, `blocked`),
acknowledgements (`ack-display`, `ack-honor`, `ack-batch`, `ack-quiet`), addressing
(`extra-topic`, `self-guard`, `skip-version`, `bad-bus`, `scoped-write`) and delegation
(`delegate`).

## Gotchas

- **Nothing is on `PATH` in a checkout.** `companion exited with 1 and said nothing` means the
  binary was not found. Set `MSGQ_COMPANION`, or `MSGQ_BIN` for the wire.
- **A herdr tab does not inherit your shell's environment.** It gets the herdr server's
  environment. `companion spawn` therefore passes `MSGQ_DB`, `MSGQ_BIN`, `MSGQ_COMPANION`,
  `MSGQ_SPAWN_EXTRA` and `MSGQ_PARENT` into the tab explicitly. Anything else a companion needs
  must be passed the same way.
- **Companions need presence.** `spawn` blocks until the new session registers on the bus, because
  a fresh consumer starts at the end of its stream and a message sent a moment too early would be
  skipped as history. A session started with `--msgq-presence off` can never pass that gate.
- **A companion must run with `honor`, and is told to trust only its parent.** Without the
  allowlist, any process that can write the bus could command a shell-capable agent.
- **Project trust may prompt.** Spawning into a directory with protected `.pi` resources is an
  interactive prompt; pass `--trust` to approve it or `--no-trust` to skip those resources.
- **A message that asks a question is delivered with an instruction to answer** using the `peers`
  tool. Without it, a peer answers into its own transcript and the asker waits in vain.
- **A failed spawn leaves its tab open**, on purpose, so you can look at what went wrong.
