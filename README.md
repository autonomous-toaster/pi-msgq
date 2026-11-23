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

## Getting started

A session another session can talk to, one command per step.

```bash
git clone https://github.com/autonomous-toaster/pi-msgq && cd pi-msgq
cargo install --path cli                   # the bus, as `msgq`
(cd extension && npm install)              # the native binding the extension reads the bus with
msgq keygen                                # this machine's signing key, and its fingerprint
msgq trust add --self                      # anchor that key, so your own records read as trusted
pi install "$PWD/extension"                # load the extension in every session from now on
export MSGQ_POLICY=honor                   # put this in ~/.zshrc: sessions honour their messages
```

Then open two sessions and name them. The name *is* the address:

```bash
pi --name alpha                            # one terminal
pi --name beta                             # another
```

`MSGQ_POLICY=honor` is what makes a session answerable, for every session started from that shell.
The default, `display`, shows incoming messages and never starts a turn; `--msgq-policy honor`
overrides the environment for one invocation. A misspelled value is refused rather than quietly
ignored — the extension stays off until it is fixed.

Under `honor`, anything that can write the bus can start a turn in your session, and a `command`
record lands as input. Narrow it with `MSGQ_ALLOW=session/alpha,session/beta` — comma-separated
sender addresses, and no list at all means any sender. (Shell publishes speak as `cli/<you>@<host>`,
so a list of sessions keeps them display-only.) For knowing *who* sent something, use the trust
layer, which `MSGQ_ALLOW` is deliberately not.

Either session can reach the other with its `peers` tool, and a shell can reach them both:

```bash
msgq sessions                              # who is live, and where
msgq publish session/beta "the suite is green"
```

That message appears in beta's transcript. Nothing else is configured: no daemon, no port, no
service — `~/.pi/agent/msgq/msgq.db` is the bus, and every client opens that same file.

Two more things worth the keystrokes:

- **`msgq keygen` before `msgq trust add --self`.** The first publish would create the key anyway,
  but `trust add --self` anchors a key that must already exist, and keygen prints the fingerprint
  you compare with a peer out of band.
- **Trust is per bus.** Anchors sit beside the keyring, keyed by the database, so pointing `--db`
  elsewhere means trusting again there; `msgq trust` names the bus and the file. And
  `pi -e /path/to/pi-msgq/extension` loads the extension for one invocation instead of the
  `pi install` above.

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
| bodies | sealed to their recipients ([sealing](#sealing-a-body-to-a-recipient)) | the same |

Nothing synchronises the two: there is no relay, no bridge and no dual write. Pick one and point
every session at it.

```bash
export MSGQ_DB=./msgq.db                                  # local: one file
msgq publish session/standup "hi"

export MSGQ_DB_URI=postgres://bus@db.internal/msgq        # shared: one server
msgq publish session/standup "hi"
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
`session/<scope>/presence`, so two teams on one server do not read each other's participants.

### Getting started with SQLite

Five minutes, one file, two sessions on the same machine.

**1. A file.** It is created on first use, owner-only (`0600`):

```bash
export MSGQ_DB=~/msgq.db
msgq publish session/standup "hello"
```

Nothing else is needed: no server, no schema, no migration. `msgq db info` says what the file is
and whether it was created by this command.

**2. Who is who: file permissions.** This backend has no identities and no credential — that is the
whole of its trust model, and it is worth saying plainly rather than implying more:

- anyone who can read the file can read the envelope but not a sealed body; seal to a recipient and
  the bodies are sealed, but the envelope, the addresses and the senders stay readable;
- anyone who can write the file can publish anything, **including a `from` that is not theirs**. A
  record's sender on this backend is a claim, not a fact.

So the honest setup is one account, or a group you already trust, and a `chmod 600` you keep. If two
users must not read each other's mail, this is not the backend to use — the shared one has identities
for exactly that reason.

**3. Point a session at it.** The same variable both sides read:

```bash
export MSGQ_DB=~/msgq.db
export MSGQ_SCOPE=standup      # optional: presence and the roster become session/standup/presence
```

**4. Check it before trusting it.**

```bash
msgq db info      # backend: local, credential: none (the local bus trusts file permissions …)
msgq publish session/standup "hello"
msgq read session/standup --from-start
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
psql "$MSGQ_DB_URI" -tAc "SELECT msgq.register_identity('alice', ARRAY['session/alice', 'session/standup'])"
# → the token, printed once. Put it in ~/.config/msgq/token, chmod 600.
```

`session/standup` covers everything beneath it, and nothing outside it. A record may speak as any
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
                    # identity: alice (scopes: session/standup, session/alice), retention_s: 3600
msgq publish session/standup "hello"
msgq read session/standup --from-start
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

## Sealing a body to a recipient

Both engines store what they are given. A body can be sealed to the people who should read it, and
the envelope stays readable so routing, the roster and acknowledgements keep working.

```bash
msgq publish session/alice-1 "the plan is tomorrow"   # sealed to alice, and to you
msgq read session/alice-1 --from-start --json | jq -c .record | msgq open --json
```

- **A record is sealed to its recipient and its sender**, so a session reads back what it sent. The
  body is sealed once with a fresh content key; that key is wrapped once per recipient, under a key
  agreed with that recipient and its own ephemeral X25519.
- **The keys come from the file you already have.** The seed in `~/.config/msgq/keys/<principal>.key`
  derives the ed25519 key that signs and, under its own label, the X25519 key that agrees. One file,
  one fingerprint to compare.
- **Every signed record carries its sender's agreement key** in `meta.sig.agree`, inside the bytes the
  signature covers — so a key is found on any record that principal wrote. No directory, it works
  with the peer offline, and substituting the key breaks the signature.
- **`msgq open`** answers one outcome per record: the opened record, the recipients it is sealed to, or
  an algorithm this build lacks. A record sealed to somebody else is a fact about the record, not a
  failure of the read. `read` already opens what it can, so this verb is for bytes obtained
  elsewhere — which is exactly the extension's case, and why it is the extension that calls it.
- **A party address is always sealed.** A `session/...` or `cli/...` address is sealed to it and to its
  sender; `--seal-to <address>` seals to a named participant as well. A publish to a party whose
  agreement key is unknown fails, naming the address: a participant advertises its key on its presence
  record, so an unknown key means nobody is there. There is no plaintext escape and no fallback.
- **Replacing a key archives it** as `<principal>.<date>.key.retired`, because a record sealed under it
  stays sealed under it forever; `msgq open` tries the archive, so history keeps opening after a
  rotation.

### One scheme, no other

There is no passphrase scheme any more. `MSGQ_PASSPHRASE`, `MSGQ_PASSPHRASE_FILE` and `MSGQ_KEY_SALT` are
not read: a key is a principal's own, or it is agreed for one record. A body is sealed to its recipients
or it is not sealed at all, and the only unsealed address is the roster.

## Trusting a sender

A bus can say who *wrote* a record — on a shared server it checks the caller's scopes and stamps the
sender itself. It cannot say who a sender *is*, because whoever runs the database could stamp
anything. So the reader decides, and it decides from a key it was given out of band:

```bash
msgq keygen                              # a key for this machine; prints its fingerprint
msgq fingerprint                         # the short form a peer compares
msgq trust add alice SHA256:9fA2...       # pin a peer you have spoken to
msgq trust                               # what is trusted here, and where each anchor came from
msgq trust rotate alice SHA256:1c3d...    # replace a key; the old one stops being believed
msgq trust revoke alice
msgq verify                              # verdicts for records handed in on stdin
```

Every record is printed with a verdict beside it — `trusted`, `unverified`, or `refused` with a
reason — and the record itself stays exactly the bytes the bus stored:

```json
{ "topic": "session/standup", "offset": 4, "created_at": 1763...,
  "verdict": { "state": "unverified", "fingerprint": "SHA256:...", "reason": "no anchor for this key" },
  "record": { "v": 1, "type": "prompt", "from": "session/alice-1", "to": "session/standup", ... } }
```

- `unverified` is a settled state, not a defect: a peer you have not pinned is still readable, and
  stays unverified however many times it speaks. Only `msgq trust` changes that, and nothing on the
  bus can ask for it.
- Trust is recorded against one bus. An anchor for the shared server does not travel to a file bus,
  and `msgq trust` names the bus it is talking about.
- Three layers decide, most transient first: `MSGQ_TRUST` for one invocation, the pi session's own
  anchors, then the keyring on disk (`~/.config/msgq/trust/<bus-id>/anchors.json`). When two layers
  name different fingerprints for one principal, the more transient one is used and the disagreement
  is reported — never resolved quietly.
- `msgq trust rotate` counts the retained records it makes unreadable, because rotation is the one
  trust act that destroys value: a record signed by a retired key is `refused`, not quietly demoted.
- `MSGQ_INSECURE=1` permits reading records that carry no signature at all, for one invocation.
  Records with no usable signature are refused by default, and nothing is exempt: presence and
  acknowledgements are signed too, because they are what a reader routes and acknowledges on.
- A client that reads the bus itself — the pi extension does — asks `msgq verify` for the verdict
  rather than carrying its own copy of the check, so one implementation decides what a record is:

  ```bash
  msgq read session/standup --from-start --json | jq -c .record | msgq verify --json
  ```

- **Trusting yourself** needs no out-of-band channel: `msgq trust add --self` reads the key this
  machine signs with, records it, and prints the principal and fingerprint so you can compare them. A
  fingerprint naming any other key is refused — a session whose own records read unverified is a
  mistake dressed as a decision. Anchors are per bus, so pin yourself on each bus you use.

One thing this deliberately does not do: tell two sessions of the same principal apart. A key belongs
to a principal, so a sibling session that holds the same key can still speak as its neighbour. Trust
is about peers, not about yourself.

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
| A message appears but the agent ignores it | the default `display` policy — a dimmed record was shown, not acted on. Start with `--msgq-policy honor` |
| A peer does nothing when messaged | its allowlist excludes you, or its policy is `display` |
| `msgq disabled` in the session | the bus could not be opened — check `MSGQ_DB` |
| `no credential was presented` | the server has identities; set `MSGQ_TOKEN` or write the identity file |
| `is outside the scopes of` | the credential may not write, read or prune that address |
| `invalid value ... possible values` | a closed-set argument refused a value; exit 2, nothing written |
| `belongs to … not …` | a prune of an address another principal opened |
| `the body does not open with this key` | a record sealed to you was altered, or your key is not the one it names |
| `sealed to session/…` | the record is addressed to somebody else; `msgq open` says who |
| `no agreement key known for …` | nobody is at that address: a participant advertises its key on its presence record |
| `the body does not open with this key` | a record sealed to you was altered, or your key is not the one that opens it |
| `unverified` beside a message | nobody pinned that key; `msgq trust add` or `msgq trust add --self` |
| `read-only` in `/peers me` | the session can read but its writes are refused |
| `msgq exited with 1 and said nothing` | it is not on `PATH`. See [DEVELOPMENT.md](DEVELOPMENT.md) |
