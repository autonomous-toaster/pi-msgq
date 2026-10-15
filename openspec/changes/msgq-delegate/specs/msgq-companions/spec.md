## ADDED Requirements

### Requirement: Trusted sender set
The command SHALL give the companion the senders it may act on, as a set the caller controls and
which defaults to the parent alone. The companion SHALL act only on senders in that set, so a
message from anybody else is shown and starts no turn.

#### Scenario: The parent is the default
- **WHEN** a companion is spawned without naming any trusted senders
- **THEN** its parent is the only sender it acts on

#### Scenario: The caller widens it
- **WHEN** a companion is spawned with additional trusted senders
- **THEN** it acts on each of them as well as on its parent

#### Scenario: A sender outside the set
- **WHEN** a companion receives a message from a sender it does not trust
- **THEN** the message is shown and no turn starts from it

#### Scenario: Following is not trusting
- **WHEN** a companion is given an address to follow but not to trust
- **THEN** it receives that address's messages and acts on none of them

## MODIFIED Requirements

### Requirement: Companion depth
A spawned companion SHALL be told its delegation depth and the cap that applies to it, so it can tell
whether it may delegate before trying; and the command SHALL refuse to spawn beyond that cap unless
the caller explicitly overrides it.

#### Scenario: Depth is propagated
- **WHEN** a companion is spawned
- **THEN** it is told a depth greater than its parent's

#### Scenario: The cap is propagated
- **WHEN** a companion is spawned
- **THEN** it is told the cap alongside its depth, rather than discovering it by being refused

#### Scenario: Cap enforced
- **WHEN** a companion at the cap tries to spawn another
- **THEN** the command refuses unless the override is given
