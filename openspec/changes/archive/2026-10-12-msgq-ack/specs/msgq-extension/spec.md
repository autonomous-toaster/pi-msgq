## ADDED Requirements

### Requirement: Acknowledgement lifecycle
When a sender requests acknowledgement, the extension SHALL emit an acknowledgement for a record at
the points it observes: on taking responsibility for the record, when a run that includes it
starts, and when that run settles. An acknowledgement SHALL name its phase, which SHALL be one of
`received`, `started`, or `completed`. A `received` acknowledgement SHALL report how the record was
delivered, and SHALL be terminal when that delivery was display-only. A `completed` acknowledgement
SHALL report the run outcome. No acknowledgement SHALL follow a terminal one.

#### Scenario: Received on responsibility
- **WHEN** the extension takes responsibility for an acknowledged record
- **THEN** it emits a `received` acknowledgement naming the delivery mode

#### Scenario: Display delivery is terminal
- **WHEN** an acknowledged record is delivered display-only
- **THEN** its `received` acknowledgement is the last one for it

#### Scenario: Started and completed
- **WHEN** a run that includes an acknowledged record starts and later settles
- **THEN** a `started` acknowledgement is emitted when it starts, and a `completed` acknowledgement
  naming the outcome when it settles

#### Scenario: A coalesced batch is acknowledged per record
- **WHEN** several acknowledged records are injected together as one coalesced batch
- **THEN** each of them is acknowledged on its own, correlated by its own offset

#### Scenario: No request, no acknowledgement
- **WHEN** a record does not request acknowledgement
- **THEN** no acknowledgement is emitted for it

### Requirement: Acknowledgement addressing
An acknowledgement SHALL be addressed to the record's sender, SHALL carry the original record's
offset as its reply reference, and SHALL identify the acknowledging session as its sender. The
combination of reply reference, acknowledging sender, and phase SHALL identify an acknowledgement
so a reader can fold duplicates. An acknowledgement SHALL never itself be acknowledged.

#### Scenario: Addressed to the sender
- **WHEN** a record sent from `session/planner` is acknowledged
- **THEN** the acknowledgement is published to `session/planner`

#### Scenario: Correlated by offset
- **WHEN** an acknowledgement is emitted
- **THEN** it carries the original record's offset as its reply reference

#### Scenario: Duplicates are recognisable
- **WHEN** one session acknowledges the same phase for one record more than once
- **THEN** a reader can recognise those acknowledgements as the same one

#### Scenario: No acknowledgement of an acknowledgement
- **WHEN** an acknowledgement is received
- **THEN** it is never itself acknowledged

### Requirement: Acknowledgement resilience
A failure to publish an acknowledgement SHALL NOT stop message consumption and SHALL NOT crash the
session. A record that cannot be decoded SHALL NOT be acknowledged. An acknowledgement SHALL NOT
start a turn.

#### Scenario: Publish failure
- **WHEN** an acknowledgement cannot be published
- **THEN** consumption continues and the condition is reported where the session has a UI

#### Scenario: Undecodable record
- **WHEN** a record cannot be decoded
- **THEN** no acknowledgement is emitted for it

#### Scenario: Notification only
- **WHEN** an acknowledgement is delivered to a session
- **THEN** it never starts a turn

## MODIFIED Requirements

### Requirement: Cursor durability
The extension SHALL persist the consumer offset once it has taken responsibility for a record, so
an interruption redelivers work that was not yet acted on rather than losing it. Responsibility is
taken when a record is shown, when it starts a turn, and when it is injected as part of a coalesced
batch. A consumer with no saved offset SHALL start at the current end of its stream.

#### Scenario: Save before report
- **WHEN** the extension takes responsibility for a record
- **THEN** its offset is persisted before that record is acted on

#### Scenario: Interrupted before responsibility
- **WHEN** the extension stops while a record is held but not yet acted on
- **THEN** that record is delivered again on the next start

#### Scenario: No history replay on first attach
- **WHEN** a session subscribes to an address that already has records
- **THEN** previously published records are not replayed

### Requirement: Scoped write exception
The extension SHALL write presence leases and acknowledgement records only, and SHALL NOT publish
any other message record to the bus. Its presence writes SHALL NOT alter the message stream,
consumer cursors, or the record contract.

#### Scenario: No message publishing
- **WHEN** the extension runs
- **THEN** no message record is published except an acknowledgement

#### Scenario: Streams untouched
- **WHEN** presence is registered, renewed, or released
- **THEN** no stream offset and no consumer cursor changes as a result

#### Scenario: No arbitrary publishing
- **WHEN** the extension handles a record it was asked to acknowledge
- **THEN** the record it publishes is an acknowledgement and nothing else
