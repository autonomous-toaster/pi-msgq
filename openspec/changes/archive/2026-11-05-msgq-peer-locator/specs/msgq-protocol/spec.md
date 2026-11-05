## ADDED Requirements

### Requirement: Presence may carry a locator card

A presence record MAY carry an optional `event.card` object that locates the participant: the project
and, where available, the git branch, worktree, and path it is working in. The field is additive, so
a `v: 1` reader that does not know it MUST still accept the record. The card SHALL be a flat object
whose reference-client fields are `project`, `branch`, `worktree`, and `path`; it SHALL NOT carry
`state`, current task, progress, or a work summary.

- `project` is the stable discovery key: a normalized `origin` remote in `host/owner/repo` form when
  one exists, otherwise the repository root directory name, or the current directory name outside
  git.
- `branch` is the current git branch when one exists and is omitted when detached.
- `worktree` is the worktree root directory name and is omitted outside git.
- `path` is the current directory relative to the repository root by default, may be absolute when
  the publisher opts in, and is omitted outside git or when path reporting is disabled.

#### Scenario: A locator card is accepted

- **WHEN** a presence record carries `event.card` with locator fields
- **THEN** a `v: 1` reader accepts the record and exposes the card without changing presence state or lifetime

#### Scenario: No card is still valid

- **WHEN** a presence record carries no `card`
- **THEN** the record is accepted exactly as before

#### Scenario: Unknown card keys are tolerated

- **WHEN** a card carries a key the reader does not know
- **THEN** the reader ignores that key for display and does not reject the record

#### Scenario: The card is not a profile

- **WHEN** a card is published
- **THEN** it does not carry current state, task, progress, summary, capabilities, version, provider, or documentation fields

#### Scenario: Path can be omitted

- **WHEN** a publisher disables path reporting or is outside git in relative mode
- **THEN** the card remains valid without `path`
