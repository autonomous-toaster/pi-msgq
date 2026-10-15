## ADDED Requirements

### Requirement: Session identity for the model
The extension SHALL report this session's own bus identity to the model when it lists the peers,
before the roster: its own address, its delivery policy, the addresses it follows, the senders it
trusts, the recipient it defaults to when one is configured, and — when the session was started with
one — its delegation depth together with the cap that applies to it. Every fact SHALL come from the
session's live configuration, and a fact that is not configured SHALL be reported as absent rather
than omitted.

#### Scenario: Which address is its own
- **WHEN** the model lists the peers
- **THEN** its own address is named, so it can tell that address from the others on the roster

#### Scenario: What it will obey
- **WHEN** the model lists the peers and a trusted sender set is configured
- **THEN** the senders it trusts are named

#### Scenario: It obeys nobody in particular
- **WHEN** the model lists the peers and no trusted sender set is configured
- **THEN** it is told there is no allowlist in force, rather than being shown an empty list

#### Scenario: Why nothing acts on a message
- **WHEN** the model lists the peers and the policy is `display`
- **THEN** the policy is named, so a message that arrived without starting a turn is explained

#### Scenario: A delegate is told whether it may delegate
- **WHEN** the model lists the peers in a session started with a delegation depth and a cap
- **THEN** both are named

#### Scenario: A session nobody delegated to
- **WHEN** the model lists the peers in a session with no delegation depth
- **THEN** the depth is reported as absent rather than as zero

## MODIFIED Requirements

### Requirement: Shipped skill
The extension SHALL make every skill it ships available to any session that loads it, so the model is
told both how a process outside the session reaches this one and how this session reaches others. A
skill SHALL be advertised by name and description without its instructions being loaded, and loading
the extension SHALL NOT depend on any skill being present or valid.

#### Scenario: The skill is advertised
- **WHEN** a session loads the extension and lists its available skills
- **THEN** the skill for being reached from outside and the skill for delegating to others are both offered by name and description

#### Scenario: The instructions load on demand
- **WHEN** the extension is loaded
- **THEN** only each skill's name and description are added, and its instructions are read when the task calls for them

#### Scenario: A broken skill does not break the extension
- **WHEN** a skill directory is missing or malformed
- **THEN** the extension still loads, consumes, and delivers

### Requirement: Session identity report
`/peers me` SHALL report this session's own identity instead of a list of others: its bus address,
the bus location, the addresses it subscribes to, its delivery policy, whether presence is
registered, the senders it trusts, the recipient it defaults to when one is configured, and its
delegation depth together with the cap that applies to it when it has one. It SHALL report these
from the session's live configuration, and SHALL report the remaining facts even when the bus cannot
be read. It SHALL be composed by the same formatter that reports the identity to the model, so the
two cannot drift apart.

#### Scenario: The address
- **WHEN** a human runs `/peers me`
- **THEN** the session's own bus address is shown

#### Scenario: What it listens to
- **WHEN** a human runs `/peers me`
- **THEN** the addresses it subscribes to are shown

#### Scenario: Policy and trust
- **WHEN** a human runs `/peers me`
- **THEN** the delivery policy and the trusted sender set are shown

#### Scenario: Presence
- **WHEN** a human runs `/peers me`
- **THEN** whether presence is registered is shown, and it is shown as off when presence is disabled

#### Scenario: The recipient it reports to
- **WHEN** a human runs `/peers me` in a session with a configured recipient
- **THEN** that recipient is shown, since it is where the session's messages go by default

#### Scenario: A delegated session
- **WHEN** a human runs `/peers me` in a session started with a delegation depth and a cap
- **THEN** both are shown, since they decide whether it may delegate further

#### Scenario: Unreadable bus
- **WHEN** the bus cannot be read
- **THEN** every fact the session already knows is still shown
