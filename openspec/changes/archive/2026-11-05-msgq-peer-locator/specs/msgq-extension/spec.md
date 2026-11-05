## ADDED Requirements

### Requirement: Presence locator card

A live session SHALL derive a locator card and publish it with its presence when presence is enabled
and locator context is not disabled. The card SHALL contain only `project`, `branch`, `worktree`,
and `path`, with the meanings defined by the presence convention. The session SHALL refresh the
derived card when the working directory changes and periodically while it runs, and SHALL publish a
new presence record only when the card actually changes. A failure to derive or publish the card
SHALL NOT stop message consumption or prevent presence without a card.

The session SHALL accept configuration for whether context is derived and for the path mode. The
serialized card SHALL be capped at 512 bytes; a card that would exceed the cap SHALL be omitted while
presence continues.

#### Scenario: Card is published

- **WHEN** a session starts with presence and context derivation enabled in a git repository
- **THEN** its presence record carries the project, branch, worktree, and configured path

#### Scenario: Context disabled

- **WHEN** context derivation is disabled
- **THEN** presence is still registered and no card is published

#### Scenario: Derivation fails

- **WHEN** the locator cannot be derived or the CLI is unavailable
- **THEN** presence is published without a card and consumption continues

#### Scenario: Working directory changes

- **WHEN** the session's working directory changes
- **THEN** the derived card is refreshed and a new presence record is published when it differs

#### Scenario: Oversized card

- **WHEN** a card would serialize to more than 512 bytes
- **THEN** the card is omitted and presence remains registered

### Requirement: Locator card action

The agent-facing peer tool SHALL accept a `card` action whose optional card input is limited to
`project`, `branch`, `worktree`, and `path`. With no card, the action SHALL report the current card.
With a card, it SHALL merge only those keys, re-derive the remaining fields, publish the updated
presence record, and report the result. A null value SHALL remove that override and return the field
to its derived value. The action SHALL NOT accept arbitrary metadata, current state, or work
summaries, and SHALL NOT publish a bus message.

#### Scenario: Read the card

- **WHEN** the model calls the card action without a card
- **THEN** it receives the current locator card

#### Scenario: Override a locator field

- **WHEN** the model calls the card action with a project value
- **THEN** the project override is applied, the other fields are derived, and presence is republished

#### Scenario: Clear an override

- **WHEN** the model calls the card action with a null field
- **THEN** that override is removed and the derived value is used

#### Scenario: Arbitrary metadata is refused

- **WHEN** the model supplies a key outside the four locator fields
- **THEN** the action refuses it instead of publishing it
