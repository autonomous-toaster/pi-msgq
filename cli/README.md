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
# Publish to a session (a bare name is shorthand for session/<name>)
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
      1  2026-10-10T21:27:40Z  session/build.done          prompt     cli/you@host  tests are green
      2  2026-10-10T21:27:45Z  session/build.done          prompt     cli/you@host  deploy finished
```

## Concepts

### Addresses

An address is the recipient **and** the stream it is written to. There is exactly one routing
concept; a message's `to` field always equals the stream name.

```
  scheme            example                       meaning
  ----------------  ----------------------------  -------------------------------
  session/<id>      session/api-worker            one pi session's inbox
  cli/<who>         cli/alice@mac                 a non-pi producer
  bus/presence      bus/presence                  the roster: everyone on the bus
  <bare-name>       reviewer                      shorthand for session/reviewer
```

If an address contains `/`, its prefix must be one of those three. Addresses are case-sensitive.
Empty addresses and scheme-only addresses (`session/`) are rejected. `group/` and `cwd/` no longer
exist: an address names one party, or it names the bus, and never a set of them.

A bare name means **`session/<name>`**, which is what `/peer reviewer` in the extension already meant.

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
| `sessions` | list live participants, from the presence records they publish; `--wide` shows their locator card |
| `card [--cwd PATH] [--paths relative\|full\|none] [--address ADDR]` | print the locator derived from a working directory |
| `await <OFFSET>` | wait for acknowledgements of a published message |
| `ask <ADDRESS> [MESSAGE]` | ask a peer a question and wait for the answer |
| `consumers [--topic <T>]` | list consumer cursors |
| `consumers rm <NAME> [--topic <T>]` | remove consumer cursors |
| `consumers gc [--apply]` | collect the cursors of sessions that are no longer live |
| `cursor get <CONSUMER> <TOPIC>` | print a saved offset, or `0` |
| `cursor set <CONSUMER> <TOPIC> <OFFSET>` | save an offset (monotonic) |
| `cursor reset <CONSUMER> <TOPIC>` | clear an offset |
| `keygen [PRINCIPAL] [--force]` | create this machine's signing key; never touches a bus |
| `fingerprint [PRINCIPAL]` | print a key's `SHA256:` fingerprint, to compare out of band |
| `trust [list\|add\|revoke\|rotate]` | record, rotate, revoke and show what this reader believes about a peer; `add --self` pins this machine |
| `verify [--json]` | reach a verdict about records handed in on standard input, one per line |
| `open [--json]` | open the body of records handed in on standard input, or say who they are sealed to |
| `db info` | path, size, contents, protocol version, per-consumer lag, record ages |
| `prune --older-than <AGE> [--apply]` | remove records older than an age, never one a consumer has not passed |

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

`--json` is an **input** format for `publish` and an **output** format for everything that prints a
record, a listing or a report, one JSON object per line, so it pipes straight into `jq`.

Every command that prints a whole stored record prints the same shape, as `read` does here:

```json
{"topic":"session/planner","offset":42,"created_at":1760000000,
 "verdict":{"state":"unverified","fingerprint":"SHA256:...","reason":"no anchor for this key"},
 "record":{"v":1,"type":"prompt","from":"session/planner"}}
```

- **`record`** is exactly what the bus stored, byte for byte. Nothing a reader adds appears inside it.
- **`offset`** is what builds a record id (`<address>#<offset>`) for `reply_to`, without reading the
database yourself.
- **`verdict`** is the reader's opinion of the sender: `trusted`, `unverified`, or `refused` with a
reason. See *Trust* below.

`topics`, `sessions`, `card`, `consumers`, `cursor` and `db info` print their own objects, unchanged.

## Sealing to a recipient

A body can be sealed to the people who should read it, rather than to the whole bus. Each principal's key file yields two keys — the one that signs and a second that agrees a
content key — so one file and one fingerprint cover both.

```bash
msgq publish session/alice-1 "the plan is tomorrow"   # sealed to alice, and to you
msgq read session/alice-1 --from-start --json | jq -c .record | msgq open --json
```

- A record is sealed to its recipient **and its sender**, so a session can read back what it sent.
- The body is sealed once with a fresh content key; that key is wrapped once per recipient, each wrap
  under its own ephemeral X25519. A record does not grow with its audience by more than about 124
  bytes per reader.
- Every signed record carries its sender's agreement key in `meta.sig.agree`, inside the bytes the
  signature covers. That is where a key is found: no directory, and it works with the peer offline.
  Substituting it breaks the signature.
- `msgq open` reads records on stdin and answers one outcome per line: the opened record, the
  recipients it is sealed to, or an algorithm this build lacks. A record sealed to somebody else is a
  fact about the record, not a failure of the read. `read` opens what it can on its own, so this verb
  exists for bytes a caller obtained elsewhere — the extension is the caller that needs it.
- A publish to a `session/` or `cli/` address is sealed to it; a bare name means the same session,
  and there is no set-naming address left for a bus-wide key to serve.
- A publish to a party whose agreement key is unknown fails, naming the address: a peer advertises its
  key on its presence record, so an unknown key means nobody is there. There is no fallback and no
  `--plaintext`, and no passphrase scheme behind either.
- Replacing a key archives the one it replaces (`<principal>.<date>.key.retired`), because a record
  sealed under it stays sealed under it forever. `msgq open` tries the archive, so history keeps
  opening after a rotation.

## Trust

The envelope says who *wrote* a record. On a shared bus that is the bus's own stamp, applied after a
scope check, so it is only as honest as whoever runs the database. A reader that needs more verifies
a signature against a key it was given out of band:

```bash
msgq keygen                              # a key for this machine, and its fingerprint
msgq fingerprint                         # the short form a peer compares
msgq trust add alice SHA256:9fA2c3d4...  # pin a peer you have spoken to
msgq trust                               # what is trusted, and where each anchor came from
msgq trust rotate alice SHA256:1b7e...   # replace a key; reports what that refuses
msgq trust revoke alice
```

- A publish signs with the principal's key: on a shared bus the identity the credential belongs to,
  otherwise `user@host`. A key is created on the first publish and reported on standard error, so
  the offset on standard output stays parseable.
- `msgq trust` never invents an anchor. Recording requires a principal and a fingerprint typed by a
  person, and one key belongs to one principal: recording somebody else's key is refused and names
  who already holds it.
- `msgq trust add --self [<fingerprint>]` is the one exception, and only because there is no other
  party to be wrong about: it names the principal this machine signs as and reads the fingerprint
  from that key, printing both. A fingerprint naming any other key is refused, so the flag cannot
  become a way to pin a peer without saying whose key it is.
- Recording over a current key is refused rather than done silently; `trust rotate` is that act, and
  it moves the old fingerprint to a retired list, reports how many retained records it makes refused,
  and keeps the retired fingerprint so a refusal can say why.
- Trust is scoped to one bus, named in `msgq trust` and in `db info`. The keyring lives at
  `~/.config/msgq/trust/<bus-id>/anchors.json`, where the bus id is a readable slug plus a digest of
  the canonical location.

Three layers decide, most transient first:

| Layer | Where it comes from | Lives for |
|---|---|---|
| `MSGQ_TRUST` | `principal:fingerprint`, comma- or space-separated | one invocation |
| `MSGQ_TRUST_SESSION` | the pi session's own anchors | the conversation |
| keyring | `msgq trust add` | until revoked or rotated |

A malformed entry and one principal given two fingerprints are both fatal: skipping either would turn
a typo into "trusted nobody", or a conflict into a coin flip. When two layers name different
fingerprints for one principal, the more transient wins and the disagreement is printed. Neither the
environment nor a session ever writes the keyring.

### Verdicts

Every record printed by `read`, `follow` or `ask` carries a verdict beside it: `trusted` when the
signature verifies against a current anchor, `unverified` when there is no usable signature or no
anchor for the key, and `refused` when the signature is broken or the key was retired. The rules, in
order:

1. A broken signature is refused whatever the anchors say: a record whose bytes changed is not the
   record that was signed.
2. A signature from a current anchor is trusted.
3. A signature from a retired key is refused, naming the date of retirement.
4. Anything else is unverified, and stays unverified however many times it speaks.

Verification happens before a sealed body is opened, because the signature covers the bytes that are
stored rather than the plaintext they become.

A record with no usable signature is refused on read unless the invocation sets `MSGQ_INSECURE=1`.
Nothing is exempt: presence and acknowledgements are signed too, because a reader routes on them. A
refused record is still printed, with its reason, and the read exits non-zero.

`msgq verify` answers the same question for a client that reads the bus itself, so there is one
implementation of the check rather than two:

```bash
msgq verify --json <<'JSON'
{"v":1,"type":"prompt","from":"session/a","to":"session/b","message":"hi"}
JSON
# {"fingerprint":null,"reason":"no usable signature","state":"unverified"}
```

It reads one record per line and prints one verdict per line, in order; a line that is not a record
is reported as unverified with a reason and does not stop the run.

## Exit codes

| Code | Meaning |
|---|---|
| `0` | success |
| `1` | runtime or database error |
| `2` | usage error (bad flag, bad address, invalid record) |

## Storage

The bus location is resolved in this order:

1. `--db <PATH-OR-URI>`
2. `MSGQ_DB_URI` — a connection string, selecting the shared engine
3. `MSGQ_DB` — a path; a connection string here is still honoured
4. `MSGQ_PGHOST` / `MSGQ_PGDATABASE`, which name a server by its settings
5. `~/.pi/agent/msgq/msgq.db`

A local file is created and initialized on first use. When `msgq` creates it, it is restricted to
owner-only permissions (`0600`); a pre-existing file is left untouched.

The bus is built on [honker](https://github.com/russellromney/honker)'s durable streams: an
indexed append-only table plus a per-consumer offset table. `msgq` adds one table of its own,
`msgq_meta`, recording the bus protocol version. Engine use is confined to one module so the
record contract and address grammar stay independent of it.

## Presence

Live participants advertise themselves with a short **record** on the presence address
(`bus/presence`): renewed while they run, retracted when they stop. `msgq sessions` reads them.

```text
$ msgq sessions
session/api-worker  session  busy  api-worker  27
session/01a12cc5    session  idle             30

$ msgq sessions --wide
session/api-worker  session  busy  api-worker  27  github.com/org/repo  main  src
```

Columns are address, kind, state, and remaining lifetime in seconds. `--wide` appends one locator
column, `project branch path`; the default output is unchanged. The identity is already in the
address and the worktree usually repeats the repository name inside `project`, so neither is
repeated; a `path` of `.` means the repository root and is left out, and a participant with no card
shows `-`. `--json` emits one object per participant and includes a `card` object with the four
stored fields when the newest presence record has one.

What the locator adds over the address line is `project` and `branch` — which repository a peer is
working in, and on which branch — which is what tells two sessions apart when several are named
alike.

A presence record can carry a small **locator card** so an agent can find a peer by project rather
than by session id:

```json
"card": {
  "project": "github.com/org/repo",
  "branch": "main",
  "worktree": "repo",
  "path": "src"
}
```

`project` is the discovery key: a normalized `origin` remote when one exists, otherwise the
repository root directory name. `branch`, `worktree`, and `path` are omitted when git or path
reporting does not apply. The card is **not** a profile: it carries no state, current task, or work
summary, and it is capped by the reference clients at 512 bytes.

Use `msgq card` to derive the same card for a script or another client:

```bash
msgq card --cwd "$PWD" --paths relative
msgq card --cwd "$PWD" --paths full      # path is the absolute current directory
msgq card --cwd "$PWD" --paths none      # omit path

msgq card --cwd "$PWD" --address session/planner
```

The last form prints the address, the card, and the locator line a roster row would show, which is
the pair a client publishes:

```json
{"address":"session/planner",
 "card":{"project":"github.com/org/repo","branch":"main","worktree":"repo","path":"."},
 "locator":"github.com/org/repo  main"}
```

Presence is a record, so it is not a pi feature: any process that can publish can appear, with
nothing but this CLI.

```bash
msgq publish bus/presence --json '{
  "v": 1, "type": "event", "from": "cli/nightly-build@host", "to": "bus/presence",
  "event": {"kind": "presence", "state": "busy", "ttl": 600, "participant": "nightly-build",
            "card": {"project": "github.com/org/repo", "branch": "main", "path": "src"}}
}'
```

A participant is live while its newest record is younger than the lifetime it declares, judged by
the bus's own clock. A clean shutdown publishes `state: "gone"`, which retracts it at once; a
participant that crashes simply ages out, so there is nothing to clean up and no daemon involved.
Presence is **advisory**: it tells you who is reachable right now, but publishing to an offline
session still queues durably. The locator card is public metadata: project names, branch names,
worktree names, and paths can be read by any process that can read the bus.

## Retention

Streams grow. Two commands bound them, both of which report before they act:

```bash
msgq prune --older-than 7d              # what would go, per stream
msgq prune --older-than 7d --apply      # remove it, then compact the file
msgq consumers gc --apply               # collect dead sessions' cursors
```

`prune` never removes a record at or below a consumer's cursor — that record is still owed to
someone — and it always keeps the newest presence record of every participant, so a participant
still ages out on its own terms. `--apply` also compacts: it rewrites the file and empties the
write-ahead log, which is what actually returns the space.

`consumers gc` only considers session-shaped consumer names (`<uuid>` or `<alias>#<uuid>`): a cursor
named by a script is that script's business.

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

`--no-wait` publishes the same record and returns as soon as it is published, printing its offset.
The answer then arrives as an ordinary record on the sender's own stream, which is what a session
does with it: a session cannot afford to hold its turn open, because two sessions waiting on each
other could then never answer. A script can.

Replies are ordinary records carrying `reply_to`, so `msgq publish --reply-to <offset>` will craft
one by hand, and `--reply-to <address>#<offset>` names one exactly. In practice you rarely do: a
peer that is asked a question is told, in the message it receives, to answer with the `peers` tool.

## Non-goals

- **No pi extension.** Injection, steering, and auto-trigger are designed-for, not implemented.
- **No automatic retention.** `prune` and `consumers gc` are operator commands, deliberately: no
  daemon deletes records on its own, and nothing runs them for you.
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
