# Proposal

## Why

Two costs recur at the prompt. The first is recall: `msgq` has seventeen verbs and about sixty flags,
and the only place that vocabulary exists is `--help`, one screen away from where it is needed. The
second is lookup: the nouns the CLI takes — addresses, consumer names, topics — are enumerable, and
the only way to get one is to run `msgq topics` or `msgq sessions` and copy it out.

Both are facts the bus already knows and does not hand to the shell. A completion script closes the
first cost by construction; the second needs the CLI to be able to enumerate a name without also
counting, sorting and formatting it.

The same survey found four vocabularies the CLI enforces at runtime and hides from everyone else:
`--type`, `--paths`, `--ack` and `--older-than` accept free text, so a typo is discovered after a
publish rather than before one. Making those closed sets visible is what lets a shell offer them, and
it is worth doing whether or not a completion is ever installed.

## What Changes

- `msgq completions zsh` prints a completion script to standard output, emitted from the command
  definition rather than typed a second time, so it cannot drift from the CLI it describes.
- Addresses, consumer names and topics complete from the live bus, filtered to the caller's scopes, so
  a completion offers only what the caller may already read.
- `msgq topics --names`, `msgq sessions --names` and `msgq consumers --names` print one name per line
  with no counts, no headers and no JSON: the cheap enumeration a shell can afford on every tab.
- `--type` and `--paths` become closed sets, and `--ack` gains a value parser, so each is validated
  where it is parsed and offered where it is completed. **BREAKING**: an invalid value for a closed
  set becomes a usage error (exit 2) instead of a runtime failure (exit 1), with different wording.
- `--ack` and `--older-than` stay open to any value but gain suggestions, because both are patterns
  (`started,completed`, `6h`) rather than members of a finite set.
- Out of scope, and named so they are not read into this change: an address derived from the working
  directory, a picker command, and shells other than zsh.

## Capabilities

### New Capabilities

- `msgq-completion`: what a tab offers and what it costs — the emitted script, the enumerations behind
  it, the bound on each tab, the scope filter, and the test that proves the script still describes the
  CLI it ships with.

### Modified Capabilities

- `msgq-cli`: the `completions` verb, the `--names` form of the three listings, the closed vocabularies
  for `--type`/`--paths`/`--ack`, and the exit-code change that follows from parsing them.

## Impact

`cli/src/main.rs` (argument definitions become closed sets; a `completions` verb), a new
`cli/src/completion.rs` (the emitter and the dynamic providers), `cli/src/commands.rs` (`--names` on
three listings), `Justfile` (install the script for the dev machine), `README.md` (the two-line install
snippet and what a tab does not offer).

No new runtime dependency: the emitter builds the script from the definition `clap` already holds, so
`clap_complete` is not added. The engine, the protocol, the record format and the extension are
untouched — this is a client-side surface, and a tab is read-only apart from the store open the CLI
already performs.
