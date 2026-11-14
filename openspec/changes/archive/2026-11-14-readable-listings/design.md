# Design

## Context

See proposal.md for motivation. Three facts decide the shape:

`card::summary` already renders a card for humans: `project  branch  full-path`, joined with two
spaces, `-` when empty. It is what `sessions --wide` prints, and it is too long for a picker.

`presence_tail` reads one presence address derived from `MSGQ_SCOPE`. A roster entry therefore exists
only for participants advertising in the caller's scope; a `group/standup` topic, or a session in
another scope, has no card available at all.

`msgq topics` and `msgq sessions` are different queries — addresses that hold records, versus live
participants — but the second column of the new form answers the same question for both: who or what
is this, in as few characters as possible.

## Goals / Non-Goals

**Goals:**
- A listing whose every line fits a terminal row at 80 columns, most of the time.
- One flag, one shape, on both listings, so a picker recipe is the same for each.
- No new read path: the compact form costs what the verbose one costs.

**Non-Goals:**
- Changing `--wide`.
- Scanning presence addresses outside the caller's scope.
- A picker command, or an fzf dependency.

## Decisions

### 1. `--card` prints two columns, address first

```
session/s-7f3a2c	pi-msgq/main  pi-msgq
group/standup	-
```

Address first because a picker selects the first field and `--with-nth=2..` hides the rest. A fixed
two-column shape means `cut -f1` never needs a special case for a row without a card.

Alternatives: replacing `--wide`'s locator with the compact one would break a documented table for a
new use; a `--format` string would put a template language in a CLI that has avoided one.

### 2. The compact rendering drops the path

`summary_compact` renders `project/branch` and then the last path component:

```
{project}/{branch}  {basename}     project and branch present
{basename}                        no project or branch
{project}/{branch}                no path
-                                 nothing to say
```

The full path stays available through `--wide` and `--json`; a picker needs to tell candidates apart,
not to locate them. Two spaces separate the two halves, matching `summary` so a reader who has seen
one form recognises the other.

### 3. The roster is looked up once, by address

`sessions` already builds a map keyed by participant; the new form needs it keyed by address. One
shared helper returns `address -> card` from the same single read, and both listings use it, so
`topics --card` adds no bus traffic beyond the presence read `sessions` already performs.

### 4. Three output forms, one at a time

`--names`, `--card` and `--json` are mutually exclusive, refused by clap at parse time (exit 2).
Any pair would be a contradiction: `--names` says "address only", `--card` says "address and
description", `--json` says "everything, machine-readable".

## Risks / Trade-offs

- **A card older than the presence lifetime is invisible** → the roster filter already excludes
  expired records; a session that stopped advertising shows `-` until it renews. Honest: a stale card
  would be a guess.
- **Two sessions on one address** → the roster keeps the newest, which is what `sessions` shows too;
  the compact form inherits that rule rather than inventing one.
- **A long project or branch name still overflows** → fzf truncates the display, and the recipe says
  so; truncating in the CLI would hide characters from a pipe that may not be fzf.

## Migration Plan

Additive: no existing invocation changes shape, and `--card` did not exist before. Rollback is
ignoring the flag.
