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
| `--parent ADDRESS` | who the companion reports to, and its **only** trusted sender |
| `--model P/M` | provider/model for the companion (default: pi's own default) |
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

A named herdr agent that holds no lease on the bus is not listed: it cannot be talked to. The bus
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
  starts, and a pi argument. It is also its trusted-sender allowlist, so a companion answers its
  parent and nobody else.
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
| `COMPANION_MAX_DEPTH` | how deep companions may spawn companions (default: 1) |

The herdr server does not share your shell's environment, so these travel to the tab explicitly
rather than being inherited.

## Testing without herdr

`--dry-run` prints every command and executes nothing, including where herdr is unavailable:

```bash
companion spawn reviewer --parent session/planner --dry-run
```

## Notes

- A companion that fails to start leaves its tab open, so you can look at what went wrong.
- A companion that never finishes is left alone: close it by hand, or `companion close`.
- A directory needing project trust will prompt on startup. Pass `--trust` to approve it or
  `--no-trust` to skip its project resources deliberately.
