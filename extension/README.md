# msgq pi extension

Delivers [msgq](../cli/README.md) messages into a running pi session. It is a **consumer**:
senders use the `msgq` CLI, the extension only reads. No session is ever published to.

Load it ad hoc with `pi -e` — nothing is installed or published.

```
   msgq publish session/api-worker "build is green"
        |
        v
   ~/.pi/agent/msgq/msgq.db  -->  pi -e ./extension  -->  [ session transcript ]
```

## Prerequisites

Install the extension's dependencies once (it uses the native honker binding):

```bash
cd extension
npm install
```

The binding ships prebuilt binaries for macOS (arm64, x64) and Linux (glibc, x64/arm64).
You also need the `msgq` CLI built to send anything: `cargo build --release --manifest-path cli/Cargo.toml`.

## Load it

From the repository root:

```bash
pi -e ./extension
```

Any path works — `pi -e` accepts a directory containing `index.ts`:

```bash
pi -e /path/to/pi-msgq/extension
```

`-e` is per invocation. To load it alongside other extensions, repeat the flag:
`pi -e ./extension -e ./other`.

## How a message reaches a session

Each session subscribes to **its own address**:

```
  session name set?          address
  ------------------------   -----------------------------
  /name api-worker           session/api-worker
  (no name)                  session/<session-id>
```

Name your session so the address is memorable:

```
/name api-worker
```

Then publish to it from any terminal:

```bash
msgq publish session/api-worker "build is green" --type note
```

`msgq consumers` lists the addresses currently being followed, which is a quick way to
find the address of a session you forgot to name.

## Delivery policy

The policy **caps** what a message may do. It is the trust dial, and `display` is the default.

| Policy | Effect |
|---|---|
| `display` (default) | every record is shown in the transcript and **never starts a turn** |
| `honor` | the record's `type` acts on the session |

Record types under `honor`:

| `type` | agent idle | agent running |
|---|---|---|
| `note` | shown, no turn | shown, no turn |
| `event` | shown, no turn | shown, no turn |
| `prompt` | starts a turn | held, then delivered as one coalesced turn |
| `steer` | starts a turn | held, then delivered as one coalesced turn |
| `follow_up` | starts a turn | held, then delivered as one coalesced turn |
| `command` | runs it, as if typed | queued as follow-up work, never coalesced |

Two extra rules always apply:

- A record whose sender equals the session's own address **never** starts a turn.
- Unknown record types are shown as display-only.

A `command` record is not content: its body is a command line, delivered as **input**, so the session
runs it exactly as it would have if it had been typed — a prompt template expands, a skill loads, an
extension command runs, and anything unrecognized passes through as ordinary text. A command must
therefore be the whole message, so the sender cannot travel inside it: a note naming them is
delivered immediately before. And a command is never coalesced, because a batch is one message and a
command has to be the whole text.

Worth being plain about what that means: dispatching goes **around the model**. A trusted sender can
run an extension command's code, without anything deliberating in between, and `/skill:x` is
dispatched even where that skill is marked as belonging to a human's hands. The trusted set was
already the gate on what may act here; this makes it considerably more load-bearing.

Messages that arrive while the agent is running are **coalesced**: however many arrive
during a run, they are delivered as a single message and cause at most one more model request.

Every delivered message names its sender and the address it arrived on, so the model can
tell external messages from your own typing:

```
[msgq] from cli/alice@mac on session/api-worker
build is green
```

## Presence

While a session runs it advertises itself with a short **record** on the presence address
(`group/presence`), renewed every 10 seconds and retracted on shutdown. Anyone can then see what is
online:

```bash
msgq sessions --wide
```

```text
session/api-worker  session  busy  api-worker  27  github.com/org/repo  main  repo  src
session/01a12cc5    session  idle             30  -  -  -  -
```

The state follows the session: `idle` at rest, `busy` while a run is in progress, and
`blocked` while it waits on a prompt. A mid-session `/name` change is reflected without a
restart. The extension also publishes a small **locator card** — `project`, `branch`, `worktree`,
`path` — so a peer can be found by where it is working rather than by session id. It is derived
from the current directory and git by `msgq card`; it is not a profile and carries no current task
or work summary. The default `msgq sessions` output is unchanged; `--wide` shows the locator.

Presence is a record, not an engine primitive, and that is deliberate: any process that can publish
can appear on the roster with nothing but the CLI, which is why the listing is not a pi feature. It
is the one stream this extension appends to, and it never appears in a conversation stream.

Presence is **advisory** — it helps a sender or an operator find sessions, and it never gates
delivery. A session that crashes stops renewing, so its record ages out on its own after the
lifetime it declared; a session that shuts down cleanly retracts it. Turn it off with
`--msgq-presence off` or `MSGQ_PRESENCE=0`; consumption is unaffected either way.

## Acknowledgements

Ask to be told what happened to a message by setting `meta.ack`:

```json
{ "v": 1, "type": "prompt", "from": "cli/ci-bot", "to": "session/api-worker",
  "message": "Deploy now", "meta": { "ack": true } }
```

`meta.ack` may be `true` for every phase or a comma-separated subset such as
`"started,completed"`. The session then publishes acknowledgements back to your address:

| Phase | Meaning |
|---|---|
| `received` | responsibility taken, with the delivery mode (`turn`, `queued`, or `display`) |
| `started` | a run that includes the record began |
| `completed` | that run settled, with an outcome (`ok`, `error`, `aborted`) |

A `display` delivery is **terminal** — the record was shown, not acted on, so nothing else follows.
Records delivered together as a coalesced batch are acknowledged **separately**, each correlated by
its own offset.

Acknowledgements are notification only: they never start a turn, and an acknowledgement is never
itself acknowledged. They are also unverified, exactly like `from`, so a peer that acts on one is
trusting everyone who can write the bus.

Wait for them from a shell with `msgq publish --await` or `msgq await <offset>`.

## What a message refers to

A record's id is `<address>#<offset>`: the stream that holds it, and the offset it was assigned
there. `msgq publish --print-id` prints it, and `--reply-to` accepts one.

When a delivered record carries `reply_to`, the extension resolves the record it names and says so
before the body, quoting it when it can still be read:

```text
[msgq] from session/reviewer on session/planner
[about session/reviewer#7 from session/planner: "review this diff"]
looks fine to me
```

This is the same rule for a reply and for an acknowledgement, because both reference the same way —
so "the answer arrived" and "they finished without answering" are read the same way too. When the
referenced record is gone, the id is named without a quote and delivery continues.

Two things are deliberate. The line is in the **content**, which the model reads: a session woken
by an answer has no idea what it is being told about, and the transcript it already has is the
human's copy, not the model's. And nothing is **remembered**: the reference is resolved at delivery
straight from the bus, so a reply to a question asked from a shell, by a script, or before this
session started resolves exactly like one asked a moment ago.

## Working with other agents

One tool puts the bus in the agent's hands.

| Tool | Actions | Needs |
|---|---|---|
| `peers` | `list` who is reachable · `send` a message · `ask` a question and wait for the answer · `reply` to the message you last received · `card` read or change this session's locator | the bus only |

A peer is any other participant on the bus: another pi session, a maki session, a script, a CI job.
Nothing here starts one. Starting a process is the job of whatever you use to run sessions — a
terminal, a launcher of your own, another agent — and the bus is how the two sides talk once it is
running.

```
peers({action:"list"})                               ->  who is reachable, and who you are
peers({action:"ask", to:"session/reviewer", message:"Review this diff"})
peers({action:"reply", message:"done"})              ->  answers whoever last asked
peers({action:"card"})                               ->  this session's locator
peers({action:"card", card:{branch:"review"}})       ->  override one locator field
```

Only `project`, `branch`, `worktree`, and `path` are accepted. A `null` value clears an override and
returns that field to the derived value.

Two things make that safe to use:

- **`--msgq-trust <addresses>`** is a comma-separated allowlist. Under `honor`, a record from
  anyone outside it is delivered display-only and **cannot start a turn**. A session started with a
  narrow list is what stops a shell-capable agent from being commanded by any process that can
  write the bus. `from` is unverified, so the allowlist was never authentication; it scopes who a
  session will *routinely* obey, and the real boundary is who can write the bus file.
- A message that **asks a question** is delivered with an instruction telling the model to answer
  with the `peers` tool. Without it a peer answers into its own transcript and the asker waits in
  vain.

### An exchange always ends

Answering is a convention, not a rule: a peer is told, in the message it receives, to answer with the
`peers` tool, and nothing enforces it. So two things make sure an exchange ends in words rather than
in silence.

- **A question that goes unanswered is asked for once more.** When a run is about to stop and the
  question this session was most recently asked is still unanswered, the extension delivers one more
  entry: answer it, or reply that nothing needed reporting. Once per question, never after an answer,
  and the session says so when it happens — so how often the convention needs rescuing is visible
  from use rather than assumed.
- **What you send reports its own completion.** Sending asks to be told when the run that included
  the message ends, so a sender that chose not to wait still learns the peer finished, including when
  it finished without answering. That report arrives as a notification: it never starts a turn.

Declining is an answer. A peer that looks at a question and decides nothing needed saying has answered
correctly, and the way to say so is in the entry, so that stopping quietly is a choice rather than a
hole.

The extension still speaks nothing of its own here: it asks the agent, the agent answers, and the
reply goes out by the same path every other message this session sends takes.

### Knowing who you are

`peers({action:"list"})` reports the peers, and before them **this session's own identity** — the
same report a human gets from `/peers me`:

```
session/planner
  bus         /Users/me/.pi/agent/msgq/msgq.db
  follows     session/planner
  policy      honor
  presence    registered
  trust       session/parent
  project     github.com/org/repo
  branch      main
  worktree    repo
  path        src
```

That answers the questions an agent otherwise guesses at: which address on the roster is its own,
whether its policy will let a message act at all, and who it is allowed to obey. All of it comes
from what the session already knows, so it works when the bus does not.

The tool shells out to the `msgq` CLI, so the wire lives in one place. `MSGQ_BIN` points at it when
it is not installed on `PATH`.

### The skills it ships

Loading the extension makes two skills available.

**`msgq-notify`** tells the model how a process *outside* the session reaches this one: a shell
script, a background job, a CI hook, a watcher. It covers finding a session's address, waking it
with a message, running something detached that reports when it finishes, asking and answering,
and asking to be told that a message arrived. It also states the two rules that fail silently: a
session cannot wake itself, because a record whose sender is its own address never starts a turn;
and a session with a trust list ignores a sender outside it.

**`msgq-delegate`** is the other direction: delegating work to another session, writing a task a
fresh process can act on, when to block and when to be woken, running several delegates at once,
and what to do with an answer that arrives from another agent.

Both are advertised by name and description only; their instructions are read when a task calls for
them, and not otherwise. They live in `extension/skills/` and are declared by the `pi` manifest in
`extension/package.json`, so loading the extension is what makes them available. Either can also be
loaded on its own:

```bash
pi --skill ./extension/skills/msgq-notify
pi --skill ./extension/skills/msgq-delegate
```

## Commands for you

The tool above belongs to the agent. These three commands are yours: run them from the prompt, and
no model turn is spent.

| Command | What it does |
|---|---|
| `/peers` | who is reachable; choosing one composes a message and sends it |
| `/peer <address> [message]` | send to one peer |
| `/reply [message]` | answer the message you most recently received |

Addresses work the same way everywhere:

- **`me` is this session.** `/peers me` reports who you are on the bus: your address, the bus
  location, the addresses you follow, your policy, whether presence is registered, and who you
  trust. It answers from what the session already knows, so it still works when the bus does not —
  which is when you are most likely to ask.
- **A bare name is a session.** `/peer reviewer "..."` sends to `session/reviewer`. A name that
  already carries a scheme (`cli/ci-bot`) is used as it is. A message sent to an address nobody
  holds is still sent, and the command tells you nobody was there — a consumer that starts later
  begins at the end of its stream and would miss it.
- **`me` is ours, not the bus's.** It means nothing to `msgq` at a shell; only the commands resolve
  it.

Leave out what you do not need and you will be asked for it:

```
/peer                          choose a peer, then compose
/peer reviewer                 compose a message to reviewer
/peer reviewer "take a look"   send it as typed
/peer "take a look"            no address: choose a peer for this message
/reply                         compose an answer to the last message you received
```

A leading quote means the whole argument is the message, which is how you give a message without
naming a recipient. Abandoning the editor sends nothing.

Every command sends and returns. None of them waits for an answer, because an answer is an inbound
message and that path already exists. The transcript tells you how to answer each one:

```
msgq note
cli/alice@mac → session/api-worker

build is green

reply: /peer cli/alice@mac ...
```

That line is part of the rendering and never part of the content the model reads. What you send is
an ordinary record from `session/<self>` — the same identity your agent uses, so a peer that trusts
this session answers you both.

## Configuration

| Setting | Flag | Environment | Default |
|---|---|---|---|
| Delivery policy | `--msgq-policy` | `MSGQ_POLICY` | `display` |
| Extra addresses | `--msgq-topics` | `MSGQ_TOPICS` | none (only `session/<self>`) |
| Presence | `--msgq-presence` | `MSGQ_PRESENCE` | `on` |
| Locator context | `--msgq-context` | `MSGQ_CONTEXT` | `auto` |
| Locator path | `--msgq-card-paths` | `MSGQ_CARD_PATHS` | `relative` |
| Lease lifetime | — | `MSGQ_PRESENCE_TTL` | `30` seconds |
| Bus location | — | `MSGQ_DB` | `~/.pi/agent/msgq/msgq.db` |

Flags win over environment variables. Extra addresses are comma-separated and use the same
address grammar as the CLI (bare names become `group/<name>`).

```bash
# Notify-only, but also watch the CI group and this project.
pi -e ./extension --msgq-topics "group/ci,cwd/Users-me-proj"

# Let scripts start turns in this session.
pi -e ./extension --msgq-policy honor
```

## Examples

```bash
# Default policy: a passive notification. Safe mid-run.
msgq publish session/api-worker "deploy finished" --type note

# A project-wide broadcast to every session following cwd/<enc>
msgq publish cwd/Users-me-proj "rebase before you commit" --type note

# Let a script drive a session (requires --msgq-policy honor)
msgq publish session/api-worker "Run the test suite and fix failures" --type prompt

# Re-aim a run that is already going (requires honor)
msgq publish session/api-worker "Skip the docs, ship the fix" --type steer

# Queue work behind the current run (requires honor)
msgq publish session/api-worker "After that, update the changelog" --type follow_up

# A full custom record
msgq publish session/api-worker --json '{"v":1,"type":"note","from":"ci/bot","to":"session/api-worker","message":"build 4821 passed"}'
```

## Progress and recovery

- The consumer cursor is saved **before** a message is shown, so an interruption replays at
  most one message.
- A session that has never consumed an address starts at the **current end** — it does not
  replay a topic's retained history. A session that has consumed before resumes from its
  saved offset.
- The footer shows `msgq: N (policy)` once subscribed, and a notice lists the addresses on start.

## Troubleshooting

| Symptom | Likely cause |
|---|---|
| Nothing is delivered | wrong address; run `msgq consumers` to see what the session follows |
| A message appears but the agent ignores it | `display` policy (the default); use `--msgq-policy honor` |
| `msgq: disabled` notice | the bus could not be opened — check `MSGQ_DB` |
| A `skipped a record` notice | the record was malformed or its `v` is unsupported; consumption continues |
| Old messages do not appear on start | by design; use the CLI's `read` to inspect history |

## Notes

- **Scoped writes:** the extension writes presence records and acknowledgements only. It never publishes an arbitrary message, so it cannot start a conversation on the bus.
- Works in interactive, RPC, and print modes; terminal UI is used only when the session has one.
- Loaded via `-e`, so it is not installed and does not appear in `pi list`.
