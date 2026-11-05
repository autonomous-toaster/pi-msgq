## Context

Presence is already a convention composed from ordinary `event` records: `state`, `ttl`,
`participant`, and `alias` are parsed tolerantly by the CLI and published independently by the pi
extension and the maki plugin. Both clients already run `msgq` and have a current working directory.
The wire promises that unknown event fields are ignored, so a locator card can be added without a
version bump. See `proposal.md` for the motivation and the delta specs for the required behavior.

## Goals / Non-Goals

**Goals:**

- Make project, branch, worktree, and path available to a peer without knowing its session id.
- Keep the card small, flat, derived, and optional.
- Keep the default `msgq sessions` text output stable.
- Share one derivation implementation between the CLI, pi, and maki.

**Non-Goals:**

- A general agent profile, capability manifest, or A2A AgentCard.
- Free-form metadata or current work/state in the card.
- A new record type, address, or protocol version.
- Peer search/filter commands beyond reading `sessions --json`.

## Decisions

### D1. Four flat keys, no nesting

`card` is `{project, branch, worktree, path}`. No `where`/`vcs` nesting and no `head`.

Rationale: the flat shape is smaller in every renewal, easier to inspect in `sessions --wide`, and
easy to filter with `jq`. A nested shape only pays off when there are many VCS-specific fields, and
there are not.

Alternatives rejected: nested `where`/`vcs`; A2A-style identity fields; free-form card object.

### D2. A2A was reviewed and rejected

A2A's AgentCard describes an HTTP/gRPC service: required interfaces, capabilities, input/output
modes, skills, and signatures. A msgq peer is a local bus address, and `participant`, `alias`, and
`from` already identify it. Copying A2A's required fields would add bytes and irrelevant data to
every presence renewal while still needing a custom location field.

A future bridge can construct an A2A AgentCard from `from` plus this locator card. The presence
record itself should not carry A2A's required fields.

### D3. The CLI owns derivation

`msgq card` is the single derivation implementation. Pi calls it through `pi.exec`; maki calls it
through its existing `run()` helper. Third-party clients can call it too.

Rationale: one place to normalize remotes, read git state, and test edge cases. Duplicating git
parsing in TypeScript, Lua, and Rust would drift.

Alternatives rejected: deriving independently in each client; deriving only in the CLI and
publishing through a new `msgq presence` command.

### D4. `project` is the primary key

`project` is the normalized `origin` remote (`host/owner/repo`, trailing `.git` removed) when it
exists, otherwise the repository root directory name, otherwise the current directory name.

Rationale: a directory basename can collide across repositories; a normalized remote is the most
useful key for "which project?". The privacy cost is documented and can be reduced by not publishing
the card.

Alternative rejected: a separate `repo` field; it would duplicate `project` for most users.

### D5. Presence renewals carry the card

The newest presence record is the truth. Keeping the card on every renewal means `sessions` can read
one record per participant without merging history. The card is capped at 512 bytes.

Rationale: the alternative — a separate card event or an occasional card-bearing presence record —
requires the roster to merge records and makes card loss possible as the presence window rotates.

Risk accepted: presence records grow by four short fields. `msgq prune` remains the retention tool.

### D6. Overrides are narrow and derived values stay underneath

The `card` tool action accepts only the four locator keys. A supplied value overrides the derived one
until cleared with `null`; a context refresh re-derives the base and reapplies the overrides.

Rationale: an agent may need to correct a project outside git or override a branch, but it must not
be able to turn the card into a profile or an unbounded metadata channel.

Alternative rejected: replacing the whole card; it would let a model drop the derived context and
invent arbitrary data.

### D7. Default path mode is relative

`relative` gives the path from the repository root; `full` gives the absolute cwd; `none` omits it.
`project`, `branch`, and `worktree` remain names either way.

Rationale: relative paths preserve discovery value while avoiding the worst path leakage by default.

## Risks / Trade-offs

- **Private project names leak** → Document that presence is public; keep absolute paths opt-in.
- **Branch/worktree goes stale** → Refresh on cwd changes and on a slow cadence; the card action can
  force an update.
- **Git is missing or slow** → Derivation fails soft; presence continues without a card, and the
  periodic refresh is guarded against overlap.
- **Card growth** → Four flat fields and a 512-byte cap; document `msgq prune`.
- **Old clients** → They ignore `card`; no protocol bump is needed.

## Migration Plan

No storage migration. Deploy the CLI first so `msgq card` exists, then the pi extension and maki
plugin. Older clients simply publish no card and ignore cards from newer peers. Rollback is removing
the publishers; existing records age out by their TTL.
