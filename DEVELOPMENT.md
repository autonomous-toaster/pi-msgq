# Developing pi-msgq

Everything here assumes a checkout. Nothing is installed, and the first thing to get right is that
`msgq` and `companion` are **not on your `PATH`** — the extension finds its helpers through
environment variables instead.

For what the system *is*, read the [README](README.md) first.

## Build

```bash
cargo build --release --manifest-path cli/Cargo.toml    # cli/target/release/msgq
(cd extension && npm install)                           # the native honker binding
```

Nothing else has to be built: the extension is TypeScript that pi loads directly, and `companion` is
a shell script.

## Point the extension at the checkout

Three variables carry everything, and the extension's tools use them to find their helpers:

```bash
export MSGQ_BIN="$PWD/cli/target/release/msgq"        # the wire
export MSGQ_COMPANION="$PWD/companion/companion"      # the herdr glue
export MSGQ_SPAWN_EXTRA="-e $PWD/extension"           # what a companion should load
```

`MSGQ_SPAWN_EXTRA` is what makes the development loop work: a spawned companion runs a plain `pi`,
and this is how it is told to load the extension out of your working tree instead of from a package.

## Run a development session

```bash
pi -e ./extension --msgq-policy honor --name planner
```

`--msgq-policy honor` matters. Under the default, `display`, every record is shown and none starts a
turn, so a companion would receive work and never do it.

Then ask it for something that needs a second agent:

```text
start a companion called reviewer and ask it what this repository does
```

That is `companions({action:"spawn",name:"reviewer"})` followed by
`peers({action:"ask",to:"session/reviewer",...})`.

To watch the bus from another terminal, use the same database:

```bash
export MSGQ_DB="$PWD/msgq-dev.db"      # optional: keep development off your real bus
msgq sessions
msgq read session/reviewer --from-start
```

## Companions by hand

When a spawn misbehaves, do it without an agent in the loop:

```bash
./companion/companion spawn reviewer --parent session/planner --json
./companion/companion list
./companion/companion spawn reviewer --parent session/planner --dry-run   # print, do not run
./companion/companion close reviewer
```

`--parent` is both where the companion reports and the **only sender it will ever act on**. A
companion answers its parent and nobody else, which is what stops a message from commanding a
shell-capable agent.

## Tests

```bash
cargo test --manifest-path cli/Cargo.toml        # 38: 23 unit + 15 integration
node --test extension/test/                      # 115 unit
cargo fmt --manifest-path cli/Cargo.toml --check
cargo clippy --manifest-path cli/Cargo.toml --all-targets -- -D warnings
```

The extension tests are `.ts` and run with no build step, so the test runner needs to strip types:
Node 23.6+, or 22.6+ with `--experimental-strip-types`.

### End to end

`extension/scripts/e2e.mjs` drives **real pi sessions** in RPC mode against a temporary bus, then
inspects what the extension delivered, published, or rendered. Some scenarios need herdr; the
command scenarios do not.

```bash
export MSGQ_BIN="$PWD/cli/target/release/msgq"
export MSGQ_COMPANION="$PWD/companion/companion"
export MSGQ_SPAWN_EXTRA="-e $PWD/extension"

node extension/scripts/e2e.mjs <scenario>       # run it with no argument for the full list
node extension/scripts/e2e.mjs delegate         # an agent spawns a peer and asks it a question
```

Overrides it understands: `MSGQ_BIN`, `PI_BIN`, `MSGQ_DB`, `MSGQ_EXTENSION`.

Each scenario prints one `RESULT: PASS <name>` or `RESULT: FAIL <name>` line and exits accordingly,
so a loop over the list is a test run:

```bash
for s in $(node extension/scripts/e2e.mjs 2>&1 | sed -n 's/^usage: .*<\(.*\)>$/\1/p' | tr '|' ' '); do
  node extension/scripts/e2e.mjs "$s" | tail -1
done
```

### What the scenarios cover

| Group | Scenarios |
|---|---|
| delivery policy | `default-note`, `note-idle`, `note-busy`, `honor-prompt`, `coalesce`, `steer-mid-run` |
| addressing | `self-guard`, `extra-topic`, `skip-version`, `bad-bus`, `scoped-write` |
| presence | `presence`, `presence-off`, `rename`, `squat`, `crash-expiry`, `blocked` |
| acknowledgements | `ack-display`, `ack-honor`, `ack-batch`, `ack-quiet` |
| delegation | `delegate` |
| the commands | `command-surface`, `command-no-peers`, `command-pick-and-send`, `command-peer-direct`, `command-peer-compose`, `command-peer-pick`, `command-reply`, `command-absent-recipient`, `command-me`, `command-identity`, `command-no-dialog`, `command-without-herdr` |
| companions | `companion-list`, `command-companions` |

Two things are worth knowing when you write more:

- `command-companions` and `companion-list` put a **fake `herdr` on `PATH`** and answer
  `herdr agent list` from a fixture, so they test the herdr glue without a window manager.
- The `command-*` scenarios that open a dialog answer it by writing an `extension_ui_response`
  back over stdin. A command handler blocks on its dialog, so **its prompt response only lands
  after the dialog is answered** — answer first, `await` the response second, or the test deadlocks.

## Gotchas

- **Nothing is on `PATH` in a checkout.** `companion exited with 1 and said nothing` means the binary
  was not found: set `MSGQ_COMPANION`, or `MSGQ_BIN` for the wire. The extension prints which
  variable to set when a helper is missing.
- **A herdr tab does not inherit your shell's environment.** It gets the herdr *server's*
  environment. `companion spawn` therefore passes `MSGQ_DB`, `MSGQ_BIN`, `MSGQ_COMPANION`,
  `MSGQ_SPAWN_EXTRA` and `MSGQ_PARENT` into the tab explicitly. Anything else a companion needs has
  to be passed the same way.
- **Companions need presence.** `spawn` blocks until the new session registers on the bus, because a
  fresh consumer starts at the end of its stream and a message sent a moment too early would be
  skipped as history. A session started with `--msgq-presence off` can never pass that gate, and the
  failure is reported rather than waited out.
- **Project trust may prompt.** Spawning into a directory with protected `.pi` resources opens an
  interactive prompt. Pass `--trust` to approve it or `--no-trust` to skip those resources.
- **A failed spawn leaves its tab open**, on purpose, so you can look at what went wrong.
- **Rebuild before testing the CLI.** `MSGQ_BIN` points at
  `cli/target/release/msgq`, which goes stale the moment you change Rust and only run `cargo test`.
- **The extension writes to the bus only for presence and acknowledgements.** The tools and the
  commands shell out to `msgq` instead. If you add a way to send, keep it that way.

## Repository layout

```
cli/            Rust. The bus and its protocol.
  src/          address.rs    address grammar and canonicalization
                config.rs     bus location resolution
                record.rs     the message record contract
                store.rs      a thin wrapper over honker's durable streams
                presence.rs   lease parsing for the `sessions` listing
                commands.rs   the verbs: publish, read, follow, await, ask, sessions
                error.rs      exit-code mapping
  tests/cli.rs  integration tests, driving the real binary
extension/      TypeScript. The pi side.
  index.ts      entry point: flags, lifecycle, delivery, presence, acknowledgements
  src/          consumer.ts     the bus, subscriptions, consumer cursors
                delivery.ts     the policy decision, and the content the model reads
                render*.ts      what a human sees, kept separate from the content
                record.ts       decoding a record
                ack.ts          the acknowledgement lifecycle
                config.ts       flags, environment, addresses, session identity
                presence*.ts    the session's own lease
                tools.ts        pure planners for the agent's two tools
                commands*.ts    the human's four commands, and run.ts to execute them
  test/         unit tests for everything pure
  scripts/      e2e.mjs, the end-to-end driver
companion/      shell. herdr glue: spawn, list, close. The only file that knows herdr.
openspec/       specs, and the change history
```

## Where the behaviour is written down

`openspec/` is the source of truth, not the code:

```bash
openspec list                       # active changes (usually none)
openspec validate --all --strict    # every spec and change must pass
```

- `openspec/specs/<capability>/spec.md` — the living contract, one requirement per behaviour, with a
  scenario for each observable outcome. `msgq-cli`, `msgq-extension`, `msgq-companions`.
- `openspec/changes/archive/<date>-<change>/` — every change that has landed, with its proposal,
  design, delta specs and tasks. The *why* behind a decision lives in its `design.md`.
- `openspec/changes/<change>/` — a change in flight.

Workflow commands and skills live in `.pi/prompts/` and `.pi/skills/` (`opsx-propose`, `opsx-apply`,
`opsx-sync`, `opsx-archive`, `opsx-explore`). A behaviour change is a change directory, not a commit.
