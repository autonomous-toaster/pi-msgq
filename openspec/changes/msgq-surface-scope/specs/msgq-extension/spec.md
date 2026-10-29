## MODIFIED Requirements

### Requirement: Presence registration
A live session SHALL register presence by publishing a time-limited presence record to the bus's
presence address, and SHALL renew it while it runs. The record SHALL declare its lifetime and SHALL
be renewed often enough that a single missed renewal does not make the session appear offline, and
the session SHALL retract its presence when it shuts down. Presence registration SHALL be enabled by
default and MAY be disabled by configuration.

#### Scenario: Registered on start
- **WHEN** a session starts and presence is enabled
- **THEN** a presence record identifying that session exists in the bus

#### Scenario: Renewal keeps it live
- **WHEN** the session keeps running
- **THEN** its most recent presence record stays younger than the lifetime it declares, without
  further action

#### Scenario: Released on clean shutdown
- **WHEN** the session shuts down cleanly
- **THEN** it publishes a presence record that retracts it

#### Scenario: Crash ages out
- **WHEN** the session stops without retracting its presence
- **THEN** the record ages out on its own

#### Scenario: Presence disabled
- **WHEN** presence is disabled by configuration
- **THEN** no presence record is published and message consumption still starts

### Requirement: Presence identity
The presence record SHALL identify the session by its session identifier and SHALL carry the session
alias when one is set, using the same identity the extension already uses for its consumer cursors.
The participant SHALL be distinguishable from every other participant in the bus.

#### Scenario: Unnamed session
- **WHEN** a session has no alias
- **THEN** its presence is identified by the session identifier

#### Scenario: Aliased session
- **WHEN** a session has an alias
- **THEN** the presence record carries that alias and the session is discoverable by it

#### Scenario: Shared alias
- **WHEN** two sessions use the same alias
- **THEN** both are listed as distinct participants and neither displaces the other

### Requirement: Presence state
The presence record SHALL carry the session state, which SHALL be one of `idle`, `busy`, or
`blocked`. The state SHALL become busy while the agent is running, return to idle once the agent
settles, and become blocked while the session waits on a user-facing prompt. A state change SHALL be
published promptly rather than waiting for the next renewal. A mid-session rename SHALL be reflected
in the session's alias and address.

#### Scenario: Idle at rest
- **WHEN** the session is idle
- **THEN** its state is `idle`

#### Scenario: Busy during a run
- **WHEN** an agent run starts
- **THEN** the state becomes `busy` promptly

#### Scenario: Idle after a run
- **WHEN** the agent settles
- **THEN** the state returns to `idle`

#### Scenario: Blocked on a prompt
- **WHEN** the session waits on a user-facing prompt
- **THEN** the state becomes `blocked` and returns when the prompt closes

#### Scenario: Renamed mid-session
- **WHEN** the session is renamed while it is running
- **THEN** its presence reflects the new alias without a restart

### Requirement: Scoped write exception
The extension SHALL publish acknowledgement records and presence records only, and SHALL NOT publish
any message record to the bus. Presence records SHALL be published to the presence address and to no
other address, so that presence never appears in a stream a session reads as conversation. Neither
kind SHALL alter the record contract, and neither SHALL advance any consumer cursor other than the
extension's own where it consumes them.

#### Scenario: No message publishing
- **WHEN** the extension runs
- **THEN** no message record is published except an acknowledgement

#### Scenario: Streams untouched
- **WHEN** presence is registered, renewed, retracted, or changes state
- **THEN** the only stream appended to is the presence address: no conversation stream offset and no consumer cursor changes as a result

#### Scenario: No arbitrary publishing
- **WHEN** the extension handles a record it was asked to acknowledge
- **THEN** the record it publishes is an acknowledgement and nothing else

### Requirement: Presence resilience
A failure to publish, renew, or retract presence SHALL NOT stop message consumption and SHALL NOT
crash the session. The extension SHALL retry on the next renewal, SHALL treat an unpublished presence
as absent rather than current, and SHALL report the condition where the session has a UI.

#### Scenario: Renewal lost
- **WHEN** a renewal cannot be published
- **THEN** the extension keeps consuming and retries at the next renewal

#### Scenario: Acquire fails
- **WHEN** the first presence record cannot be published
- **THEN** consumption still starts and the condition is reported

#### Scenario: Lease squatted
- **WHEN** another process publishes presence for this session's address
- **THEN** consumption continues, the entry is not silently doubled, and the condition is reported

#### Scenario: Presence is not claimed when it was not published
- **WHEN** a publication is not confirmed
- **THEN** the session does not describe itself as registered
