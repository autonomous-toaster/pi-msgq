# Proposal

## Why

`msgq topics --names | fzf` picks the right address but shows `session/s-7f3a2c`, which says nothing
about which session that is. The card that would say it — project, branch, working directory — is
already on the bus, published as presence, and only `msgq sessions --wide` reads it.

The obvious fix, piping `--wide`, prints lines that are too long for a picker: the locator joins
project, branch and the full path, so one candidate fills the terminal and fzf scrolls horizontally.

## What Changes

- `msgq topics --card` and `msgq sessions --card` print two tab-separated columns: the address, then a
  compact description — `project/branch  basename` — never the full path.
- An address no participant advertises prints `-` in the second column, so the shape never changes.
- `--card` conflicts with `--json` and with `--names`: three output forms, chosen one at a time.
- README gains the picking recipes for both listings, with the fzf flags that hide the second column
  from the display and still select on the first.

`--wide` is unchanged: it stays the human table, and this adds the machine-adjacent form rather than
reshaping an existing one.

## Capabilities

### New Capabilities

None: this adds a form to listings the CLI capability already owns.

### Modified Capabilities

- `msgq-cli`: the listings gain a card form — two columns, a compact locator, and the fallback when no
  participant advertises the address.

## Impact

`cli/src/commands.rs` (two listings, one shared roster lookup), `cli/src/card.rs` (a compact summary
beside the existing one), `cli/src/cli.rs` (a flag on two commands), `README.md` (the recipes).
No engine, protocol or extension change: the compact form is a rendering of data already on the bus,
and it is read through the same scoped calls the listings already make.
