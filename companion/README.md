# companion

Start and stop companion agents: a real `pi` process in its own herdr tab, wired to the
[msgq](../cli/README.md) message bus so the two ends can talk.

```
   companion spawn reviewer        ->  a new herdr tab running pi, reachable at session/reviewer
   msgq ask session/reviewer "..." ->  the task is just the first message
   companion list                  ->  what is running, and where
   companion close reviewer        ->  the tab is gone
```

There is no separate task argument, no protocol, and no daemon. Spawning creates a process; the
work is a message like any other.

This command is one implementation of a contract, not a feature of the bus. It is the showcase: the
thing that shows two independently-owned agents on one file, with a person able to type into either
one. Nothing on the wire depends on it, and a tmux script, a different multiplexer, or a durable
harness can satisfy the same contract — see [The launcher contract](#the-launcher-contract).

## Requirements

- **herdr**, and you must be running inside it (`HERDR_ENV=1`). Companions are herdr tabs.
- **jq**, for reading herdr's JSON.
- **msgq** on `PATH`, or `MSGQ_BIN` pointing at it. The readiness gate and name de-duplication
  consult the bus.

## Usage

```
companion spawn <name> [options]      start a companion
companion list [--json]               list the companions that are running
companion close <name>                close its tab
companion help                        this text
```

| Option | Meaning |
|---|---|
| `--cwd PATH` | where the companion runs (default: your working directory) |
| `--workspace ID` | herdr workspace (default: yours) |
| `--parent ADDRESS` | who the companion reports to, and its default trusted sender |
| `--accept ADDRESSES` | extra senders it will act on, comma-separated |
| `--model P/M` | model for the companion: `id`, `provider/id`, or `id:thinking` (default: pi's own) |
| `--provider NAME` | restrict `--model` lookup to one provider; needs `--model` |
| `--thinking LEVEL` | `off`, `minimal`, `low`, `medium`, `high`, `xhigh`, or `max` |
| `--trust` / `--no-trust` | grant or withhold project trust for that directory |
| `--topics LIST` | extra bus addresses for it to follow |
| `--allow-depth` | override the delegation depth cap |
| `--focus` | focus the new tab (default: leave your focus alone) |
| `--timeout SECONDS` | how long to wait for it to become reachable |
| `--json` | emit the result as one JSON object |
| `--dry-run` | print the commands instead of running them |

The result names how to reach it:

```json
{ "name": "reviewer", "address": "session/reviewer",
  "cwd": "/path/to/repo", "tab": "w1:t3", "pane": "w1:p5" }
```

`companion list` answers the other direction — what is running now. It joins herdr's agents to
the bus by the name they share, so an entry is a tab that is both alive and listening:

```
  reviewer  working  /path/to/repo
```

```json
{ "companions": [ { "name": "reviewer", "address": "session/reviewer",
                    "status": "working", "cwd": "/path/to/repo" } ] }
```

A named herdr agent that is not on the roster is not listed: it cannot be talked to. The bus
roster (`msgq sessions`) is the other half of that picture — it lists everyone reachable, whether
or not they live in a herdr tab.

## How a companion is wired

```bash
herdr tab create --label reviewer --cwd "$PWD" --no-focus \
    --env MSGQ_PARENT=session/planner --env MSGQ_DEPTH=1 --env MSGQ_DB=...
herdr agent start reviewer --kind pi --pane <root_pane> -- \
    --name reviewer --msgq-policy honor \
    --msgq-report-to session/planner --msgq-trust session/planner
```

- The **name is one string** in four places: the herdr agent, the pi session name, and therefore
  the address `session/<name>`. The command de-duplicates it, so a second `reviewer` becomes
  `reviewer-2`, and the address follows whatever was granted.
- **`--parent` is both channels**: an environment variable, inherited by anything the companion
  starts, and a pi argument. It is also its default trusted sender.
- **`--accept` widens who it obeys**, and defaults to nobody but the parent:

  ```bash
  companion spawn reviewer --parent session/planner --accept cli/ci-bot@host
  ```

  The companion acts on its parent and on `cli/ci-bot@host`, and on nobody else — another
  companion, an unrelated session, or a group it merely follows with `--topics` is shown and never
  acted on. Worth being precise about what that is: `from` is unverified, so this set was never
  authentication. It scopes what a shell-capable agent will *routinely* obey, and the real boundary
  is who can write the bus file. Widen it narrowly.
- **The model is a choice, not a default.** `--model lab/qwen3-30b` (or `provider/id`, or
  `id:thinking`) is passed to the companion's pi, so a reviewer can run on a cheaper or a different
  model than the session that spawns it. `--provider` only narrows where a model id is looked up, so
  it needs `--model`; the command refuses it alone rather than letting pi fail inside a tab.
  Credentials are not passed: a provider the companion can use is one pi already knows about, from
  `models.json` or a stored key.
- **How hard it thinks is a choice too.** `--thinking high` reaches the companion's pi, and needs no
  model of its own: a level applies to whatever model runs and is clamped to what that model
  supports. `--model id:high` still works and is overridden by `--thinking`; a level pi does not
  know is refused here, where the message can list them.
- **Depth and the cap both travel**, as `MSGQ_DEPTH` and `COMPANION_MAX_DEPTH`, so a delegate can
  tell whether it may delegate rather than discovering it by being refused.
- Spawning **blocks until the companion registers on the bus**. A new consumer starts at the end of
  its stream, so a message sent before its subscription existed would be skipped as history.
  Presence is only registered after the subscription, so it is proof, not a guess.

## Environment

| Variable | Purpose |
|---|---|
| `MSGQ_BIN` | the msgq binary (default: `msgq` on `PATH`) |
| `MSGQ_DB` | the bus, passed through to the companion |
| `MSGQ_SPAWN_EXTRA` | extra pi arguments, e.g. `-e /path/to/extension` during development |
| `MSGQ_DEPTH` | the caller's delegation depth, set by spawn for the child |
| `COMPANION_MAX_DEPTH` | how deep companions may spawn companions (default: 1), passed to the child with the depth |

The herdr server does not share your shell's environment, so these travel to the tab explicitly
rather than being inherited.

## The launcher contract

Starting a companion is a shape any launcher can produce. What it has to do:

1. **Start a `pi` process** that loads the extension (`-e <extension>`, or a package that does) in a
   working directory of the caller's choosing, in a place a human can watch and type into, on the
   model the caller asked for (`--model`, `--provider`) and at the thinking level they asked for
   (`--thinking`) when either was named.
2. **Name it once.** The name is the herdr agent, the pi session name, and the bus address
   `session/<name>`. A taken name is de-duplicated, and the granted name is what gets reported.
3. **Tell it who it reports to and whom it obeys**: `--msgq-report-to <parent>`, and
   `--msgq-trust <parent>[,<accept>...]`.
4. **Give it the bus and its tools**: `MSGQ_DB`, `MSGQ_BIN`, `MSGQ_COMPANION`,
   `MSGQ_SPAWN_EXTRA`, `MSGQ_PARENT`, `MSGQ_DEPTH`, `COMPANION_MAX_DEPTH` — passed explicitly, since
   a launcher's server does not share the caller's environment.
5. **Ask for a turn-taking policy**: `--msgq-policy honor`, or the companion is delivered messages
   and does nothing with them.
6. **Wait until it is reachable**: poll `msgq sessions --json` for `session/<name>` before returning,
   because a new consumer starts at the end of its stream and an early message would be skipped.
7. **Be able to stop it**, and leave a failed start visible rather than reaped.

Everything in this README is one way to satisfy that list. `--dry-run` prints the list as commands:

```bash
companion spawn reviewer --parent session/planner --dry-run
```

```text
herdr tab create --label reviewer --cwd /work/repo --no-focus --workspace w3K \
  --env MSGQ_DEPTH=1 --env COMPANION_MAX_DEPTH=1 --env MSGQ_DB=/Users/me/.pi/agent/msgq/msgq.db \
  --env MSGQ_BIN=/usr/local/bin/msgq --env MSGQ_COMPANION=/usr/local/bin/companion \
  --env MSGQ_SPAWN_EXTRA=-e\ /work/pi-msgq/extension --env MSGQ_PARENT=session/planner
herdr agent start reviewer --kind pi --pane \<root_pane\> -- --name reviewer --msgq-policy honor \
  --msgq-report-to session/planner --msgq-trust session/planner -e /work/pi-msgq/extension
# then wait until session/reviewer appears on the bus
```

## Notes

- A companion that fails to start leaves its tab open, so you can look at what went wrong.
- A companion that never finishes is left alone: close it by hand, or `companion close`.
- A directory needing project trust will prompt on startup. Pass `--trust` to approve it or
  `--no-trust` to skip its project resources deliberately.
