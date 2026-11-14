# Design

## Context

See proposal.md for motivation. Three facts about the current state shape every decision below:

`cli/src/main.rs` holds the whole CLI as `clap` derive types: `Cli`, `Command`, `ConsumersCmd`,
`CursorCmd`, `DbCmd`. Nothing else in the repository knows the command tree, so an emitter that walks
`Cli::command()` cannot disagree with the parser about which flags exist.

Four arguments already have a closed set that only the code knows: `record::KNOWN_TYPES` (six values),
`card::PathMode::parse` (`relative`, `full`, `none`), the acknowledgement phases, and the age syntax.
`clap` is not told about any of them, so `--help` does not list them and a typo fails at run time.

The enumerable nouns are already there: `commands::topics`, `commands::sessions` and
`commands::consumers` each list what a shell would want to offer, and since `tenant-isolation` each
listing is filtered by the caller's scopes. What is missing is a form that prints only the name: the
current forms print counts, lags and lost totals, which cost a query per row and cannot be pasted.

`msgq card` derives a locator from a directory and touches no store, but no CLI command derives an
address from a directory — that rule lives in the extension alone.

## Goals / Non-Goals

**Goals:**
- One description of the CLI: the completion script is generated, never transcribed.
- A tab offers what the caller could already read, and nothing else.
- A tab is bounded: one enumeration, no counting, no formatting.
- The vocabulary the CLI enforces is visible in `--help` and in the completion.

**Non-Goals:**
- Shells other than zsh.
- An address derived from the working directory (see Decisions).
- A picker command, an fzf binding, or any interactive selection.
- Changing what a default listing prints.

## Decisions

### 1. Emit the script from `clap`'s tree, not from `clap_complete`

`clap_complete` writes a correct zsh script, but it bakes each value spec into the generated
`_arguments` call. A dynamic value cannot be injected afterwards without rewriting that generated
text, which is the same drift this change exists to prevent, with an extra step.

Emitting from `Cli::command()` costs roughly a hundred and fifty lines and buys two things: the static
part cannot drift, and the dynamic part can be *declared* in Rust next to the command definitions
instead of inside a shell string. The alternative — a hand-written `_msgq` checked against `--help` by
a test — is the only other way to get dynamic values, and it trades a generator for a test that must
be written anyway.

A new module, `cli/src/completion.rs`, holds the emitter and the provider table. No dependency is
added.

### 2. Two kinds of value, because two kinds exist

```
closed set (from the definition)     restrict: only members complete
    --type, --paths, <subcommands>

pattern value (declared as hinted)   suggest: offered, but any text is accepted
    --ack (started,completed), --older-than (6h), --reply-to (addr#offset)
```

zsh needs different machinery for each: a closed set completes through the value spec itself, so a
non-member is silently dropped; a hinted value must offer completions while leaving the cursor free,
which is `compadd -S ''` rather than a value spec. Collapsing the two would either make `--older-than`
reject `3h` or make `--type` accept nonsense, and in both cases the CLI would then have to re-validate
what it never promised to validate.

### 3. Dynamic providers are declared, keyed by command path and argument id

```rust
// (command path, argument) -> enumeration
("read", "addresses")            -> Addresses
("follow", "addresses")          -> Addresses
("publish", "address")           -> Addresses
("consumers", "--topic")         -> Topics
("cursor get", "consumer")       -> Consumers
("cursor get", "topic")          -> CursorTopics
("card", "--cwd")                -> Files("directories")
("", "--db")                     -> Files("files")
```

The emitter looks each key up while walking the tree, so a key that names a command or argument the
CLI does not have is a bug the test finds without running zsh: the coverage test walks
`Cli::command()` and asserts every provider key resolves, and every subcommand, long flag and closed
set in the tree appears in the emitted text. That is the honest half. The other half — that zsh parses
the output — is a `zsh -n` check, which is a lint, not a conformance case.

### 4. One enumeration per tab, and `--names` is that enumeration

`msgq topics --names` reuses `list_topics` but prints one address and skips the per-address count. On
the local engine that is one SQLite query; on the shared engine it is the catalogue call, because the
count moved into the catalogue only for the default listing's benefit. The alternative — completing
from `--json` and piping through `jq` — would put a parser in the shell script and another dependency
in the README.

`--names` conflicts with `--json`, which `clap` refuses at parse time rather than printing something
that is neither.

No caching in zsh. A cache is a file write and a stale address, for a saving that a local tab does not
need and a shared tab does not notice.

### 5. A tab opens the bus, and that is accepted

`Store::open` creates `~/.pi/agent/msgq/msgq.db` when the file is absent, and on a shared engine it
bootstraps `msgq.meta`, `msgq.address`, `msgq.cursor`, `msgq.retention` and `msgq.identity` with
`CREATE ... IF NOT EXISTS`. A tab therefore creates a bus the first time it runs in a shell with no
`MSGQ_DB`.

Accepted, because the alternative is a no-create open threaded through both engines and every command,
and because the effect is what starting any `msgq` command would do anyway. The completion passes no
`--db` of its own, so it enumerates the bus the caller's environment names, and the README says that a
tab creates the default bus if nothing else is configured.

### 6. The directory-derived address is not in this change

The rule that turns a working directory into `cwd/<enc>` lives in `extension/src`, not in the CLI. To
offer it on a tab means writing that rule a second time in Rust, and the one precedent for a
duplicated rule in this repository — `queue_name`, in both Rust and SQL — survives only because a
parity test compares the two.

So v1 completes addresses that exist. If the contextual address is wanted later, it is a
`msgq address [--cwd PATH]` command with a parity fixture against the extension, which is a change of
its own and does not alter the completion's shape.

### 7. Closed sets move into `clap`, which changes an exit code

`--type` becomes a `ValueEnum` over `record::KNOWN_TYPES`, `--paths` over `PathMode`, and `--ack`
gains a value parser. The visible consequence is that a non-member becomes a usage error: exit 2 with
`clap`'s wording, instead of exit 1 after a partial run. That is a CLI contract change and the reason
the `msgq-cli` delta carries it as its own requirement rather than as a detail of completion.

The vocabulary stays defined where it is used: `ValueEnum` is derived on a type that wraps
`KNOWN_TYPES`, so `record.rs` remains the only place the six types are listed.

## Risks / Trade-offs

- **The emitter grows with `clap`'s feature set** → the coverage test walks the tree, so a construct
  the emitter ignores shows up as a missing flag in the emitted text rather than as a silent omission;
  adding a provider key that does not resolve fails the same test.
- **Quoting inside the generated script** → descriptions and member lists are escaped once, in the
  emitter, and a description carrying a quote or a newline is a test case rather than a review note.
- **`msgq sessions --names` reads presence records** → it is the same read `msgq sessions` performs,
  so a bus with many presence records is slower to complete than a bus with many addresses. Bounded by
  the existing lifetime filter; documented rather than capped, because a cap would hide a participant.
- **A tab is not free on a shared engine** → one round trip. Documented; the alternative was caching.
- **zsh only** → stated in the help, the spec and the README, so no one discovers it by trying bash.
- **`--names` becomes public API** → it is small, it is the form every other unix tool has, and it is
  what makes `msgq read $(msgq topics --names | fzf)` work without a picker command.

## Migration Plan

`--type` and `--paths` reject what they used to accept-then-fail. A script that relied on the old
error text or on exit 1 for a bad value must accept exit 2 and `clap`'s wording; nothing that used a
valid value changes. The completion itself is additive: no existing invocation changes, and installing
`_msgq` is opt-in. Rollback is deleting the script from `fpath`.

## Open Questions

- Whether a second shell is worth emitting. The emitter is a walk over the `clap` tree, so a bash or
  fish backend is mechanical; the question is whether an untested backend is better than an absent
  one, and it can be answered after zsh has been used for a while.
