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
msgq sessions --wide   # who is online, where they work, and what they are doing
msgq consumers     # which addresses are being followed, how far behind, and what they lost
msgq read session/planner --from-start    # the retained history
```

History is a **window**, not an archive: records are retained for one hour by default
(`MSGQ_RETENTION=6h`, or `msgq prune --retention 6h`). `msgq prune` removes what is older and names the
consumers that will lose records; `msgq consumers` reports each consumer's lag and how many records
retention already took from it; `msgq read --strict` refuses to print a partial view instead of
warning about the gap.

## Ask, and get an answer back

`msgq ask` publishes a question and waits. It ends on a reply, on a terminal acknowledgement, or on
its timeout — and a timeout is not an error.

```bash
msgq ask session/planner "which tests should I run?" --from session/me
```

Any record can ask to be told what happened to it, and you can wait for that separately:

```bash
msgq publish session/planner "run the suite" --type prompt --await
msgq await session/planner#42           # by record id: the wait finds who to watch
msgq await 42 --on cli/ci-bot           # by offset, when you know where the acks land
```

An offset alone names no record — offsets are unique only within a stream — so `await` takes a
record id, or an offset together with `--on`. Acknowledgements are addressed to the sender of the
record being awaited, which is not the stream the record was published to; the record id form looks
that sender up for you.

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
| `peers` | `list` who is reachable · `send` a message · `ask` a question · `reply` to the message you last received |

A peer is any other participant: another pi session, a maki session, a script, a CI job. The tool
does not start one — that is the job of whatever runs your sessions. It finds them, talks to them,
and hands work over:

```text
ask session/reviewer to review the parser, and tell me what it said
```

Under the hood that is `peers({action:"ask",to:"session/reviewer",...})`: the question is published
and the turn ends, and the answer arrives as a message that starts the next one. `wait:true` takes
the answer inside the same turn instead — bounded, because a turn that waits cannot answer anything
arriving behind it, and two sessions waiting on each other would then never finish.

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
and bus state. Every record `msgq read`/`follow` prints carries its topic, its offset and its trust
verdict beside it, which is what lets the agent name a record for a reply.

Two maki instances can share `name`: each sends as its own `session/<session-id>`, listens there
as well, and follows with a consumer of `maki#<session-id>`, so a shared alias fans out to all of
them instead of colliding on the follow lock. Address the id for one session, the alias for the
group; answers always come back from the id.

## Two backends

One product, two engines, and a deployment runs exactly one of them:

| | local | shared |
|---|---|---|
| location | a filesystem path: `MSGQ_DB`, `--db` | a connection string: `MSGQ_DB_URI`, or `MSGQ_PG*` settings |
| storage | one SQLite file | PostgreSQL, through [pgmq](https://github.com/pgmq/pgmq) |
| reach | the machine that holds the file | every machine that can reach the server |
| identity | file permissions; the sender is a claim | a credential the server checks |
| retention | a declared window, default one hour | the same window |
| bodies | plaintext unless you set a passphrase ([sealing](#sealing-message-bodies)) | the same |

Nothing synchronises the two: there is no relay, no bridge and no dual write. Pick one and point
every session at it.

```bash
export MSGQ_DB=./msgq.db                                  # local: one file
msgq publish group/standup "hi"

export MSGQ_DB_URI=postgres://bus@db.internal/msgq        # shared: one server
msgq publish group/standup "hi"
```

`--db` still takes either form when one command needs to say so explicitly. `MSGQ_DB_URI` wins over
`MSGQ_DB` when both are set, and `MSGQ_DB` still accepts a connection string so an older
configuration keeps working.

The shared engine also reads `MSGQ_PGHOST`, `MSGQ_PGPORT`, `MSGQ_PGUSER`, `MSGQ_PGPASSWORD`,
`MSGQ_PGDATABASE` and `MSGQ_PGSSLMODE`: libpq's names, under this bus's own prefix. They supply what
the connection string does not, which is how a password stays out of a URI. Naming them `MSGQ_PG*`
rather than reading `PG*` is deliberate — msgq is the client, and a shell's global PostgreSQL
settings belong to whatever else is running.

The credential is `MSGQ_TOKEN` or `~/.config/msgq/token`; `msgq db info` says whether it was
accepted, and never prints it.

Presence and the roster are per scope: set `MSGQ_SCOPE` and a session advertises on
`group/<scope>/presence`, so two teams on one server do not read each other's participants.

### Getting started with SQLite

Five minutes, one file, two sessions on the same machine.

**1. A file.** It is created on first use, owner-only (`0600`):

```bash
export MSGQ_DB=~/msgq.db
msgq publish group/standup "hello"
```

Nothing else is needed: no server, no schema, no migration. `msgq db info` says what the file is
and whether it was created by this command.

**2. Who is who: file permissions.** This backend has no identities and no credential — that is the
whole of its trust model, and it is worth saying plainly rather than implying more:

- anyone who can read the file can read every plaintext record in it; set a passphrase and
  the bodies are sealed, but the envelope, the addresses and the senders stay readable;
- anyone who can write the file can publish anything, **including a `from` that is not theirs**. A
  record's sender on this backend is a claim, not a fact.

So the honest setup is one account, or a group you already trust, and a `chmod 600` you keep. If two
users must not read each other's mail, this is not the backend to use — the shared one has identities
for exactly that reason.

**3. Point a session at it.** The same variable both sides read:

```bash
export MSGQ_DB=~/msgq.db
export MSGQ_SCOPE=standup      # optional: presence and the roster become group/standup/presence
```

**4. Check it before trusting it.**

```bash
msgq db info      # backend: local, credential: none (the local bus trusts file permissions …)
msgq publish group/standup "hello"
msgq read group/standup --from-start
msgq sessions     # who advertised themselves, in this scope
```

The `from` column in that `read` is the CLI's guess at who you are — `cli/<user>@<host>` — and the
bus stores it because nothing here can check it.

`credential:` reading `none (… the sender is a claim)` is not a defect to fix: it is the backend
telling the truth about itself. Everything above the store is the same as on a server — addresses,
offsets, retention, gap warnings, `ask`, `await` — which is why the two walkthroughs differ only in
this step.

### Getting started with PostgreSQL

Five minutes, one server, two sessions.

**1. A PostgreSQL with pgmq.** Any server you already run, with pgmq installed:

```bash
psql "$MSGQ_DB_URI" -f https://raw.githubusercontent.com/pgmq/pgmq/v1.13.1/pgmq-extension/sql/pgmq.sql
```

The bus creates its own tables (`msgq.address`, `msgq.cursor`, `msgq.identity`, …) on first
connection and creates no queue until an address is used. To try it locally without touching a real
server, `just pg-url` builds and starts one in podman and prints its URI.

**2. An identity per user.** A credential is minted once and handed out; the server keeps only its
hash. The identity names a **principal** — a user, not an address — and its scopes are the addresses
that user may reach:

```bash
psql "$MSGQ_DB_URI" -tAc "SELECT msgq.register_identity('alice', ARRAY['session/alice', 'group/standup'])"
# → the token, printed once. Put it in ~/.config/msgq/token, chmod 600.
```

`group/standup` covers everything beneath it, and nothing outside it. A record may speak as any
participant address inside those scopes — `session/alice/sub`, `cli/alice@laptop` — because one
credential serves every session of that user. A claim outside them is **refused** and stores nothing:
there is no address the bus could rewrite it to, and a room is not a sender either.

One credential, several sessions is what makes a long silence survivable: `msgq.renew_credential`
extends the same row and keeps the digest, so a session that picks up after weeks is still itself and
every sibling session stays logged in. The extension renews at session start and again at half its
lease; by hand it is one `psql` call.

**3. Point a session at it.** Three settings, once, in the shell that starts pi:

```bash
export MSGQ_DB_URI=postgres://bus@db.internal/msgq
export MSGQ_TOKEN=$(cat ~/.config/msgq/token)
export MSGQ_SCOPE=standup
```

**4. Check it before trusting it.**

```bash
msgq db info        # backend: shared, credential: accepted as alice,
                    # identity: alice (scopes: group/standup, session/alice), retention_s: 3600
msgq publish group/standup "hello"
msgq read group/standup --from-start
```

`credential: none (no identity is registered …)` means the server has no identities yet, so it
cannot authorize anything and accepts writes unauthenticated. `missing (this bus has N identities
…)` means it can, and this session did not present one. That is the whole of the trust model: once
an identity exists, an unauthenticated write is refused rather than downgraded.

**Reads, cursors and prune are scoped too.** A read, a catalog listing and a consumers listing name
only what is inside your scopes, and a cursor belongs to the principal that wrote it, so two users can
pick the same consumer name without moving each other's position. `msgq prune` removes records only
from an address you opened — ownership follows the first write, not read access, so a member of a
shared room cannot erase another member's history — and it is the only thing allowed to write the
retention watermark. The engine's own tables are not readable or writable by the runtime role at all:
every operation passes through a function that resolves the credential first.

## Sealing message bodies

Both engines store what they are given. Configure a passphrase and the **body** is sealed on the way
in and opened on the way out; the envelope is left alone so routing still works.

```bash
export MSGQ_PASSPHRASE="correct horse battery staple"    # or MSGQ_PASSPHRASE_FILE=/run/secrets/msgq
msgq publish group/standup "the plan is tomorrow"
msgq read group/standup --from-start                     # opened for you, transparently
```

- One key per principal, derived from the passphrase with scrypt. The key is never stored, so a copy
  of the bus file opens nothing; `MSGQ_KEY_SALT` names the derivation and `db info` prints the first
  bytes of its hash as the **key identifier**, which tells two keys apart without revealing either.
- `aes-256-gcm` with a fresh nonce per record. The algorithm and the identifier travel in the
  record's metadata, so a reader that does not know the algorithm preserves the record and treats the
  body as opaque rather than guessing.
- Address, sender, type and offset stay readable, because routing, the roster and acknowledgements
  need them. Presence and acknowledgement bodies are never sealed.
- A body that does not open is an error, not content: `msgq read` exits non-zero, prints no body and
  names the key identifier; the extension reports it, starts no turn and publishes no acknowledgement,
  and the session keeps running.
- A stream may hold plaintext and sealed records side by side, so turning encryption on does not
  orphan the history that came before it.

## Completion

zsh, and zsh only: the script a tab runs calls back into the CLI for the names it cannot know.

```bash
just completions            # writes ~/.zfunc/_msgq
# or, by hand, anywhere on fpath:
msgq completions zsh > "${fpath[1]}/_msgq"
```

Then, once per shell, in `~/.zshrc`:

```zsh
fpath=(~/.zfunc $fpath)
autoload -Uz compinit && compinit
```

What a tab offers: every subcommand and flag, the closed sets (`--type`, `--paths`, the shell of
`completions`), suggestions for the arguments that take a pattern (`--ack`, `prune --older-than`), and
— the part a static script cannot do — the addresses, topics and consumer names on your bus. Those
come from `msgq topics --names`, `msgq sessions --names` and `msgq consumers --names`, which is also
why those three flags exist: one enumeration per tab, no counting, no formatting.

To pick a peer or a stream by what it *is* rather than by its id, use a card form: two tab-separated
columns, the address first and a short description second (`fzf` shows the second and selects the
first).

```bash
msgq sessions --card | fzf | cut -f1   # who to message
msgq topics --card   | fzf | cut -f1   # which stream to read
```

The address comes first, so it stays visible when a narrow terminal truncates the right edge, and
`cut -f1` takes it from the line `fzf` prints. Type either half to narrow the list: both columns are
matched.

The description is `project/branch  directory`, never the full path, and each part is clipped to a
budget (`14/14`, then `10`) with the middle replaced by `…` — `gitlab-nc…estrator/63-orches…-run-api`
— so a long repository or branch name cannot crowd out the address beside it.
`msgq sessions --wide` still prints the whole locator when you want to know where a session really is,
and `-` stands in the second column for an address no live participant advertises (in this scope).

Two things a tab does not do, stated so neither is a surprise. It opens the bus, because the names
live there: with `MSGQ_DB` unset that creates `~/.pi/agent/msgq/msgq.db` the first time, exactly as
running any `msgq` command would. And it completes addresses that exist, not the address of your
working directory — that rule lives in the extension, and a second copy of it here would be a second
answer to the same question.

The script is generated from the command definition, never transcribed, and a test in the CLI suite
fails when a flag or a closed set is missing from it. Regenerate after upgrading:

```bash
just completions
```

## What it will not do

- **No daemon and no broker.** The local backend is one SQLite file and the shared backend is a
  PostgreSQL you already run; msgq adds no service of its own. Reach is the reach of the file or of
  that server, and nothing crosses between the two.
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
| `no credential was presented` | the server has identities; set `MSGQ_TOKEN` or write the identity file |
| `is outside the scopes of` | the credential may not write, read or prune that address |
| `invalid value ... possible values` | a closed-set argument refused a value; exit 2, nothing written |
| `belongs to … not …` | a prune of an address another principal opened |
| `did not open (kid=…)` | the passphrase differs from the one the record was sealed with |
| `read-only` in `/peers me` | the session can read but its writes are refused |
| `msgq exited with 1 and said nothing` | it is not on `PATH`. See [DEVELOPMENT.md](DEVELOPMENT.md) |
