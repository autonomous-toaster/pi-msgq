## ADDED Requirements

### Requirement: Presence registration
A live session SHALL register a time-limited lease in the bus and renew it while it runs. The
lease SHALL be renewed often enough that a single missed renewal does not make the session appear
offline, and SHALL be released when the session shuts down. Presence registration SHALL be
enabled by default and MAY be disabled by configuration.

#### Scenario: Registered on start
- **WHEN** a session starts and presence is enabled
- **THEN** a lease identifying that session exists in the bus

#### Scenario: Renewal keeps it live
- **WHEN** the session keeps running
- **THEN** its lease remains unexpired without further action

#### Scenario: Released on clean shutdown
- **WHEN** the session shuts down cleanly
- **THEN** its lease is released

#### Scenario: Crash ages out
- **WHEN** the session stops without releasing its lease
- **THEN** the lease expires on its own

#### Scenario: Presence disabled
- **WHEN** presence is disabled by configuration
- **THEN** no lease is created and message consumption still starts

### Requirement: Presence identity
The lease SHALL identify the session by its session identifier and SHALL carry the session alias
when one is set, using the same identity the extension already uses for its consumer cursors. The
lease SHALL be distinguishable from every other lease in the bus.

#### Scenario: Unnamed session
- **WHEN** a session has no alias
- **THEN** its lease is identified by the session identifier

#### Scenario: Aliased session
- **WHEN** a session has an alias
- **THEN** the lease carries that alias and the session is discoverable by it

#### Scenario: Shared alias
- **WHEN** two sessions use the same alias
- **THEN** they hold distinct leases and neither displaces the other

### Requirement: Presence state
The lease SHALL carry the session state, which SHALL be one of `idle`, `busy`, or `blocked`. The
state SHALL become busy while the agent is running, return to idle once the agent settles, and
become blocked while the session waits on a user-facing prompt. A state change SHALL be recorded
promptly rather than waiting for the next renewal. A mid-session rename SHALL be reflected in the
session's alias and address.

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
The extension SHALL write presence leases only, and SHALL NOT publish message records to the bus.
Its presence writes SHALL NOT alter the message stream, consumer cursors, or the record contract.

#### Scenario: No message publishing
- **WHEN** the extension runs
- **THEN** it publishes no message record

#### Scenario: Streams untouched
- **WHEN** presence is registered, renewed, or released
- **THEN** no stream offset and no consumer cursor changes as a result

### Requirement: Presence resilience
A failure to acquire, renew, or release a lease SHALL NOT stop message consumption and SHALL NOT
crash the session. The extension SHALL attempt to re-acquire a lease it no longer holds, and SHALL
report the condition where the session has a UI.

#### Scenario: Renewal lost
- **WHEN** a renewal reports that the lease is no longer held
- **THEN** the extension attempts to re-acquire and continues consuming

#### Scenario: Acquire fails
- **WHEN** the lease cannot be acquired
- **THEN** consumption still starts and the condition is reported

#### Scenario: Lease squatted
- **WHEN** another process holds the session's lease
- **THEN** consumption continues and the condition is reported
