# msgq for maki

Steer a [maki](https://github.com/earendil-works/maki) session from a shell, and let it answer
back. The plugin consumes the same [msgq](../README.md) bus the pi extension uses: one SQLite
file, no daemon, no network.

```
   you, a script, or another agent                 maki session
        |                                              ^
        |  msgq publish session/planner "..."          |  msgq follow
        v                                              |
   ~/.pi/agent/msgq/msgq.db  ------------------------>  plugin --> prompt / note
```

## Install

```bash
./maki/install.sh                 # copies lua/msgq.lua and plugin.toml
```

Then in `~/.config/maki/init.lua`:

```lua
require("msgq").setup({ name = "planner" })
```

Run `/reload` in maki. With `name = "planner"` the session answers at both `session/planner` and
its unique `session/<session-id>`:

```bash
msgq publish session/planner "run the tests and fix the failures" --ack --await
msgq ask session/planner "what does this repository do?"
```

Without `name`, the address is `session/<session-id>`; `/peers me` shows it. A `name` is an
**alias followed in addition**: the session always listens on `session/<session-id>`, so several
maki instances can share a memorable name (fan-out) and still be addressed individually.

## Configuration

`setup()` is called from `init.lua` once. Every field is optional.

| option | default | meaning |
|---|---|---|
| `name` | none | alias address, followed **in addition** to `session/<session-id>`: `session/<name>`, or a full address when it contains `/` |
| `policy` | `"honor"` | `honor` acts on the record type; `display` only shows messages and never starts a turn |
| `trust` | every sender | list (or comma string) of addresses allowed to act; others are display-only |
| `presence` | `true` | advertise on `group/presence` so `msgq sessions` lists the session |
| `bin` | `$MSGQ_BIN` or `msgq` | the CLI to run |
| `db` | `$MSGQ_DB` or the CLI default | bus location |
| `topics` | none | extra addresses to follow, e.g. `{ "group/ci" }` |
| `ttl` | `30` | presence lease, in seconds |
| `presence_every_ms` | `10000` | presence renewal interval |

```lua
require("msgq").setup({
  name = "planner",
  policy = "honor",
  trust = { "cli/alice@mac", "session/orchestrator" },
  topics = { "group/ci" },
})
```

## What a message does

Records are delivered into the session focused when the plugin started (one address per maki
process). Delivery follows the record's `type`:

| `type` | idle | running |
|---|---|---|
| `prompt`, `steer`, `follow_up` | starts a turn | queued, then delivered |
| `note` | shown, no turn | shown, no turn |
| `event` | shown as a notice | shown as a notice |
| `command` | runs it, as if typed, when this session is focused; otherwise delivered as a prompt marked "not run" | queued |
| unknown | shown, no turn | shown, no turn |

A record whose sender is this session's own address never starts a turn, which is what stops a
session waking itself. Under `trust`, a sender outside the list is delivered display-only.

Delivered content names the sender, so the model can tell the bus from its user:

```
[msgq] from cli/alice@mac on session/planner
the build is green
```

A question (`meta.reply`, which `msgq ask` sets) is delivered with the exact tool call to answer
it. A `reply_to` reference is named as an id, `[about session/alice#7]`.

## Acknowledgements

`meta.ack` is honoured: `received` (with `delivery = turn | queued | display`) immediately,
`started` on the next turn, and `completed` (with `outcome = ok | error | aborted`) when that turn
ends. `msgq publish --ack --await` therefore returns when maki finishes. Display-only deliveries
are terminal and send nothing further.

## Commands for you

| command | what it does |
|---|---|
| `/peers` | who is reachable; choose one, compose, send |
| `/peers me` | this session's address, bus, consumer, policy, trust, presence, follow job |
| `/msgq` | alias for `/peers me` |
| `/peer <address> [message]` | send to one peer; a bare name is `session/<name>`, `me` is this session |
| `/reply [message]` | answer the message received most recently |

With no message, the command opens `$EDITOR` on a draft; an empty or abandoned draft sends
nothing. Addresses that carry a scheme (`cli/`, `group/`, `cwd/`) are passed to the CLI as typed.
Sending from a command publishes as this session and asks to be told when the peer's run ends.

## The `msgq` tool

The agent gets one tool, for answering peers and starting conversations:

```
msgq({action:"list"})                     who is live
msgq({action:"send", to:"session/api", message:"build is green", type:"note"})
msgq({action:"reply", message:"looks fine to me"})
```

`reply` answers the message received most recently, carrying its `reply_to` and arriving at the
sender's address.

## Identity and presence

The session advertises itself on `group/presence` while it runs, so `msgq sessions` lists it with
`idle` / `busy` / `blocked`. The record is published from `session/<session-id>` (the unique
address) with the configured name as its alias, so several instances sharing one alias stay
distinguishable. A clean session end retracts the record; a crash ages out after the TTL.
Presence is advisory and never gates delivery.

`/peers me` answers from what the session already holds, so it still works when the bus does not:

```
session/planner
  session id  CgiccyNbdVnFfcih8JHyG
  bus         /Users/me/.pi/agent/msgq/msgq.db
  consumer    maki#CgiccyNbdVnFfcih8JHyG
  follows     session/CgiccyNbdVnFfcih8JHyG, session/planner
  policy      honor
  trust       (none set: every sender)
  presence    registered (idle)
  follow job  running (pid 1234)
  last error  (none)
```

## How it works

`msgq follow <address>… --consumer maki#<session-id> --json --with-offset` runs as a
session-scoped job, following `session/<session-id>` and any configured alias. `--with-offset`
wraps each record with `{topic, offset, created_at, record}`, which is how a reply and an
acknowledgement can name `<address>#<offset>`. The consumer is unique per session, so two maki
instances may follow the same alias and each get its own cursor instead of fighting over the
follow lock. `/reload` adopts the running job instead of starting a second one.

## Limits

- One bound session per maki process, attached to the tab focused at startup; other tabs are not
  separately addressable. It listens on its `session/<session-id>` and any configured alias.
- A restarted maki gets a new session id and therefore a new consumer cursor: it starts at the
  end of its streams, so messages published while it was down are not replayed. Resume the same
  session (`maki -r <id>`) to keep the cursor.
- Messages that arrive during a run are queued one by one, not coalesced.
- `command` records can only run in the focused tab; in a background tab they degrade to a prompt.
- Consumers named `maki#<session-id>` are not collected by `msgq consumers gc`. Reset one with
  `msgq consumers rm maki#<session-id> <address>`.
- The commands need an interactive UI; under `maki -p` the roster becomes a one-line notice and
  composing is unavailable.

## Troubleshooting

| symptom | likely cause |
|---|---|
| nothing arrives | wrong name; `/peers me` shows the addresses, `msgq consumers` shows what is followed |
| a message shows but the agent ignores it | `policy = "display"`, or the sender is outside `trust` |
| `unsupported msgq protocol version` | the record's `v` is not 1 |
| `follow line has no offset` | an older `msgq` without `--with-offset`; rebuild/install the CLI |
| follow job keeps restarting | `msgq` is missing or the database is unreadable; check `/peers me` last error |
