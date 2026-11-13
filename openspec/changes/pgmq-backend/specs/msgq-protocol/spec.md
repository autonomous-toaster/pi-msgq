# Spec Delta

## MODIFIED Requirements

### Requirement: The envelope a client may depend on

T1.1 ALWAYS, a record SHALL be a JSON object.
T1.1 ALWAYS, a record SHALL carry `v`, `type`, `from`, and `to`.
T1.1 ALWAYS, a record MAY carry `message`, `event`, `reply_to`, and `meta`.
T1.1 ALWAYS, a record id SHALL name an address together with an offset.
T1.1 ALWAYS, offsets SHALL be unique within a stream.
T1.1 ALWAYS, a client SHALL NOT assume offset uniqueness across the bus.
T1.1 ALWAYS, `reply_to` SHALL name an offset in the stream of the answering record's `from`.
T1.1 ALWAYS, `meta` SHALL be an open object.
T1.1 ALWAYS, a client SHALL preserve an unrecognized `meta` key.
T1.1 ALWAYS, a client SHALL NOT fail on an unrecognized `meta` key.
T1.1 ALWAYS, no other top-level field SHALL be required.

#### Scenario: Minimal record
- **WHEN** a client publishes with only a sender, an address and a message
- **THEN** the record SHALL carry `v`, `type`, `from`, `to` and `message`

#### Scenario: Unknown meta keys
- **WHEN** a record carries a `meta` key that a reader does not recognize
- **THEN** the reader SHALL ignore the key and SHALL preserve the key

#### Scenario: Answering by reference
- **WHEN** a record answers another record
- **THEN** the answering record SHALL carry the answered offset as `reply_to`

#### Scenario: An offset is scoped to its stream
- **WHEN** two streams each hold a record at the same offset
- **THEN** the bus SHALL name the two records by two different addresses

### Requirement: Presence is a convention over event records

T5.5 ALWAYS, presence SHALL be an `event` record with `event.kind` of `presence`.
T5.5 ALWAYS, a presence record SHALL carry a `state` and a lifetime.
T5.5 ALWAYS, presence SHALL travel to one well-known address per scope.
T5.5 ALWAYS, a participant SHALL count as live while the newest presence record is younger than the declared lifetime.
T5.5 ALWAYS, the bus SHALL judge liveness by recorded time.
T5.5 ALWAYS, a roster SHALL list only the participants of the roster scope.
T5.5 ALWAYS, a clean shutdown SHALL publish a presence record that retracts the participant.

#### Scenario: Any client can appear
- **WHEN** a process publishes a presence record with nothing but the command-line client
- **THEN** the roster SHALL list the process alongside pi sessions

#### Scenario: Retraction is immediate
- **WHEN** a participant retracts presence on clean shutdown
- **THEN** the roster SHALL drop the participant without waiting for the lifetime

#### Scenario: Crash ages out
- **WHEN** a participant stops without retracting presence
- **THEN** the roster SHALL drop the participant after the declared lifetime elapses

#### Scenario: Scopes do not see each other
- **WHEN** two participants publish presence in different scopes
- **THEN** each roster SHALL list only the participants of one scope

### Requirement: One vocabulary, checked against fixtures

T1.4 ALWAYS, one place SHALL define the record vocabulary.
T1.4 ALWAYS, every implementation SHALL pass the same fixtures.
T1.4 ALWAYS, every implementation SHALL accept the same fixtures.
T1.4 ALWAYS, every implementation SHALL reject the same fixtures.
T7.1 ALWAYS, the fixtures SHALL cover bus behaviour.

#### Scenario: Fixtures agree
- **WHEN** the conformance fixtures run against two implementations
- **THEN** both implementations SHALL accept and reject the same records

#### Scenario: Drift is caught
- **WHEN** an editor edits one implementation's known-type list and leaves the other list alone
- **THEN** a conformance test SHALL fail

#### Scenario: Engines agree on behaviour
- **WHEN** the behavioural fixtures run against each storage engine
- **THEN** both engines SHALL produce the same outcome for every fixture

## ADDED Requirements

### Requirement: The retention window is declared

T1.2 ALWAYS, the bus SHALL declare a retention window.
T1.2 ALWAYS, the bus SHALL report the retention window.

#### Scenario: The window is reportable
- **WHEN** a client asks the bus about the bus
- **THEN** the client SHALL learn the retention window

### Requirement: Retention removes older records

T1.2 AFTER the retention window elapses, the bus SHALL remove older records.
T1.2 AFTER a record exceeds the retention window, a client SHALL NOT assume persistence of the record.
T1.2 AFTER a consumer falls behind the window, the bus SHALL let the consumer lose records.

#### Scenario: History is a window, not an archive
- **WHEN** a client reads the oldest available records of an address
- **THEN** the client SHALL receive the oldest retained records

#### Scenario: A lagging consumer does not pin history
- **WHEN** a consumer position is older than the retention window
- **THEN** the bus SHALL remove records on schedule

### Requirement: The envelope is always readable

T1.5 ALWAYS, a reader SHALL read `v`, `type`, `from`, `to`, `reply_to`, and `meta` without interpreting a body.
T1.5 ALWAYS, a reader SHALL read presence bodies without interpreting opaque content.
T1.5 ALWAYS, a reader SHALL read acknowledgement bodies without interpreting opaque content.
T1.5 ALWAYS, a reader SHALL report an error for an unreadable body.
T1.5 ALWAYS, a reader SHALL NOT treat an unreadable body as content.

#### Scenario: A body may be unreadable
- **WHEN** a reader encounters a record with a body that the reader cannot interpret
- **THEN** the reader SHALL name the address, the offset, the sender, and the type

#### Scenario: An opaque body is not acted on
- **WHEN** a reader cannot interpret a record body
- **THEN** the reader SHALL report an error

#### Scenario: Acknowledgement and presence stay readable
- **WHEN** a client routes an acknowledgement or builds a roster
- **THEN** the client SHALL need no capability beyond reading the record

### Requirement: A body may declare its encoding

T1.3 ALWAYS, an encoded body SHALL carry a `meta.enc` declaration.
T1.3 ALWAYS, a client SHALL preserve an unrecognized `meta.enc` key.
T6.3 ALWAYS, a reader SHALL treat an unhonourable `meta.enc` as an opaque body.

#### Scenario: Declaration travels in meta
- **WHEN** a sender encodes a body for a reader
- **THEN** `meta.enc` SHALL name the encoding and the key identifier

#### Scenario: Ignoring the declaration is harmless
- **WHEN** a client reads a record with a `meta.enc` value that the client does not understand
- **THEN** the client SHALL preserve the key and SHALL NOT fail

### Requirement: Answering is requested in `meta.reply`

T1.3 ALWAYS, a sender that wants an answer SHALL set `meta.reply` to true.
T1.3 ALWAYS, a reader SHALL NOT treat a record without `meta.reply` as a question.
T1.3 ALWAYS, a reader SHALL NOT interpret the body of a reply request.

#### Scenario: A question is marked as one
- **WHEN** a sender publishes a record with `meta.reply` set to true
- **THEN** a reader SHALL detect the request without reading the body

#### Scenario: Ordinary records owe nothing
- **WHEN** a record carries no `meta.reply`
- **THEN** a reader SHALL NOT treat the record as a question
