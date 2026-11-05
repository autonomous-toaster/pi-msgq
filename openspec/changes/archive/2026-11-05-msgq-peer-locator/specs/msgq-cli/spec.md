## ADDED Requirements

### Requirement: Peer locator derivation

`msgq card` SHALL print one flat JSON object describing the current working directory as a peer
locator. It SHALL derive the locator from git when the directory is inside a work tree, and SHALL
succeed with a smaller object when git is unavailable or the directory is not a repository. A bad
`--cwd` value SHALL be a usage error.

The command SHALL accept `--cwd PATH` and `--paths relative|full|none`. `relative` is the default and
puts the path relative to the repository root in `path`; `full` puts the absolute current directory
there; `none` omits `path`. Outside git, `branch` and `worktree` SHALL be omitted; under `full`,
`path` SHALL still be the absolute current directory.

`project` SHALL be the normalized `origin` remote as `host/owner/repo` with any trailing `.git`
removed when an origin exists, otherwise the repository root directory name, or the current directory
name outside git.

#### Scenario: A git repository

- **WHEN** `msgq card` runs in a git repository with an origin
- **THEN** it reports the normalized origin as `project`, the current branch, the worktree name, and a relative path

#### Scenario: A subdirectory

- **WHEN** `msgq card` runs in a subdirectory of a git repository
- **THEN** `path` is relative to the repository root and `worktree` names the root directory

#### Scenario: No git repository

- **WHEN** `msgq card` runs outside git
- **THEN** it reports the current directory name as `project` and omits `branch` and `worktree`

#### Scenario: Remote normalization

- **WHEN** the origin remote is an HTTPS or SSH URL
- **THEN** `project` is the same `host/owner/repo` value for both forms

#### Scenario: Path modes

- **WHEN** `--paths` is `relative`, `full`, or `none`
- **THEN** `path` is respectively relative to the root, the absolute current directory, or absent

#### Scenario: Detached HEAD

- **WHEN** the repository is on a detached HEAD
- **THEN** the locator omits `branch` and still reports the project and worktree

## MODIFIED Requirements

### Requirement: Sessions listing
`msgq sessions` SHALL list the live participants recorded as unexpired presence records in the bus.
Each entry SHALL show the participant's address, kind, state, and remaining lifetime. Records older
than the participant's declared lifetime SHALL be excluded, at most one entry SHALL be shown per
participant, and `--json` SHALL emit one JSON object per participant. The listing SHALL be derived from
records alone, so that any participant able to publish is listed, whether or not it is a pi session.
When a participant's newest presence record carries a locator card, `--json` SHALL include that card,
and the text output SHALL show its locator fields only when `--wide` is requested. The default text
output SHALL remain unchanged.

#### Scenario: Lists a live session
- **WHEN** a participant has published a presence record that has not yet expired
- **THEN** `msgq sessions` shows its address and its state

#### Scenario: Any participant can appear
- **WHEN** a process publishes a presence record using nothing but the command-line client
- **THEN** it is listed alongside pi sessions

#### Scenario: Excludes expired leases
- **WHEN** a presence record is older than the lifetime it declares
- **THEN** that participant is not listed

#### Scenario: Retraction is immediate
- **WHEN** a participant retracts its presence before shutting down
- **THEN** it is not listed, without waiting for the lifetime to elapse

#### Scenario: One entry per participant
- **WHEN** more than one presence record exists for the same participant
- **THEN** the listing shows a single entry for it, reflecting the most active state and the newest record

#### Scenario: Kind is reported
- **WHEN** participants of more than one kind are present
- **THEN** each entry reports its kind

#### Scenario: JSON output
- **WHEN** `--json` is given
- **THEN** each participant is emitted as one JSON object per line, with the same fields as before and a `card` field when one is present

#### Scenario: Wide output
- **WHEN** `--wide` is given
- **THEN** the text output appends the project, branch, worktree, and path from the card, using a placeholder for a missing field

#### Scenario: Default text output is unchanged
- **WHEN** `--wide` is not given
- **THEN** the text output has the same columns as before, whether or not the participant has a card

#### Scenario: Nobody online
- **WHEN** no presence record is live
- **THEN** the command succeeds and lists nothing
