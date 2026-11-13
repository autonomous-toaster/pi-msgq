# Spec Delta

## MODIFIED Requirements

### Requirement: Bus access

T3.3 ALWAYS, the extension SHALL consume the bus location that the CLI consumes.
T3.3 ALWAYS, the extension SHALL read `MSGQ_DB_URI` when the variable is set.
T3.3 ALWAYS, the extension SHALL read `MSGQ_DB` when the variable is set.
T3.3 ALWAYS, the extension SHALL prefer `MSGQ_DB_URI` over `MSGQ_DB`.
T6.1 ALWAYS, exactly one backend SHALL serve one session.
T6.1 ALWAYS, the extension SHALL call the backend API for reads.
T6.1 ALWAYS, the extension SHALL call the backend API for writes.
T6.1 ALWAYS, the extension SHALL close the bus handle on shutdown.

#### Scenario: Shared location
- **WHEN** the CLI has published to the default path and the extension starts without `MSGQ_DB`
- **THEN** the extension SHALL read from that bus

#### Scenario: Environment override
- **WHEN** `MSGQ_DB` is set
- **THEN** the extension SHALL use that location for the whole session

#### Scenario: Server override
- **WHEN** `MSGQ_DB_URI` and `MSGQ_DB` are set
- **THEN** the extension SHALL use the `MSGQ_DB_URI` location for the whole session

#### Scenario: Bus created on first use
- **WHEN** the extension starts and the bus file does not exist
- **THEN** the engine SHALL create the file

### Requirement: Cursor durability

T6.2 BEFORE the extension acts on a record, the extension SHALL persist the consumer offset.
T6.2 BEFORE a consumer starts, the consumer SHALL take the current end as the first position.

#### Scenario: Save before report
- **WHEN** the extension takes responsibility for a record
- **THEN** the extension SHALL persist the record offset

#### Scenario: Interrupted before responsibility
- **WHEN** the extension stops while a record is held and not yet acted on
- **THEN** the bus SHALL deliver that record again on the next start

#### Scenario: No history replay on first attach
- **WHEN** a session subscribes to an address with existing records
- **THEN** the extension SHALL replay no earlier record

### Requirement: Delivery policy

T6.3 ALWAYS, the extension SHALL apply a subscriber policy.
T6.3 ALWAYS, the default policy SHALL be `display`.
T6.3 ALWAYS, the `display` policy SHALL deliver every record display-only.
T6.3 ALWAYS, the `display` policy SHALL start no turn.
T6.3 ALWAYS, the `honor` policy SHALL use `type` for a trusted sender.
T6.3 ALWAYS, an untrusted sender SHALL receive display-only delivery.
T6.3 ALWAYS, an unrecognized policy SHALL disable the extension.
T6.3 ALWAYS, the extension SHALL treat an uninterpretable body as an error.

#### Scenario: Display is the default
- **WHEN** no policy is configured and a record of type `prompt` arrives
- **THEN** the extension SHALL deliver a display-only message

#### Scenario: Honor enables turns
- **WHEN** the policy is `honor` and a record of type `prompt` arrives while the agent is idle
- **THEN** the extension SHALL start a turn

#### Scenario: Untrusted sender
- **WHEN** the policy is `honor`, a trusted sender set is configured, and a record arrives from a sender outside the set
- **THEN** the extension SHALL deliver the record display-only

#### Scenario: Trusted sender
- **WHEN** the policy is `honor` and a record arrives from a sender inside the trusted set
- **THEN** the extension SHALL let `type` determine delivery

#### Scenario: Unknown policy rejected
- **WHEN** configuration sets an unrecognized policy
- **THEN** the extension SHALL report the error

### Requirement: Graceful degradation

T6.4 IF the bus is unreadable, THEN the extension SHALL report the extension disabled.
T6.4 IF the credential is rejected, THEN the extension SHALL report that the session cannot write.
T6.4 IF a record carries an unsupported version, THEN the extension SHALL skip the record.
T6.4 IF a session cannot write, THEN the reported state SHALL name the credential problem.

#### Scenario: Unreadable bus
- **WHEN** the bus cannot be opened
- **THEN** the extension SHALL report that the extension is disabled

#### Scenario: Unsupported record version
- **WHEN** a record carries a protocol version the extension does not support
- **THEN** the extension SHALL skip the record

#### Scenario: Rejected credential
- **WHEN** the backend refuses the session credential
- **THEN** the session SHALL report that the session cannot write

#### Scenario: A read-only session is distinguishable
- **WHEN** a session cannot write because of the session credential
- **THEN** the reported state SHALL name the credential problem

### Requirement: Presence registration

T5.5 ALWAYS, the extension SHALL publish a presence record to the scope address.
T5.5 ALWAYS, the extension SHALL enable presence registration by default.
T5.5 ALWAYS, a running session SHALL renew the presence record.
T5.5 ALWAYS, a clean shutdown SHALL retract the presence record.
T5.5 ALWAYS, a session SHALL read the roster from the scope address.

#### Scenario: Registered on start
- **WHEN** a session starts and presence is enabled
- **THEN** a presence record identifying the session SHALL exist in the bus

#### Scenario: Renewal keeps it live
- **WHEN** the session keeps running
- **THEN** the newest presence record SHALL stay younger than the declared lifetime

#### Scenario: Released on clean shutdown
- **WHEN** the session shuts down cleanly
- **THEN** the session SHALL publish a presence record that retracts the session

#### Scenario: Crash ages out
- **WHEN** the session stops without retracting presence
- **THEN** the presence record SHALL age out

#### Scenario: Presence disabled
- **WHEN** presence is disabled by configuration
- **THEN** the extension SHALL publish no presence record

#### Scenario: Presence stays within the scope
- **WHEN** a session is configured with a scope
- **THEN** the session SHALL publish to the scope presence address

## ADDED Requirements

### Requirement: The session is woken by the backend

T4.6 IF the bus writes a record to a followed address, THEN the backend SHALL wake the session.
T4.6 IF the backend cannot push a wake, THEN the session SHALL poll.

#### Scenario: Woken rather than polled
- **WHEN** a record is written to an address the session follows while the session is idle
- **THEN** the session SHALL deliver the record before the next polling interval

#### Scenario: Polling survives an engine that cannot push
- **WHEN** the backend cannot deliver a wake on the active connection
- **THEN** the session SHALL still receive records through polling

### Requirement: The shared backend serves a session

T6.1 IF the location is a server connection string, THEN the extension SHALL select the shared backend.
T7.3 IF two sessions share the shared backend, THEN the sessions SHALL exchange a question and a reply.

#### Scenario: Shared backend selected
- **WHEN** the location is a server connection string
- **THEN** the session SHALL use the shared backend only

#### Scenario: Two sessions exchange work
- **WHEN** two sessions share one shared backend
- **THEN** each session SHALL see the presence of the other session

### Requirement: A gap is reported

T6.2 IF the saved offset is older than the retained window, THEN the extension SHALL report the lost count.
T6.2 IF the extension reports a gap, THEN the extension SHALL resume at the oldest retained record.

#### Scenario: Lost records are reported
- **WHEN** the saved offset is older than the oldest retained record
- **THEN** the extension SHALL report the number of lost records

#### Scenario: A gap does not disable the session
- **WHEN** a gap has been reported
- **THEN** the session SHALL keep consuming subsequent records

### Requirement: The extension reads the body first

T6.3 BEFORE the extension considers `type`, the extension SHALL read the record body.

#### Scenario: An uninterpretable body is an error
- **WHEN** the extension cannot interpret a record body
- **THEN** the session SHALL report an error

### Requirement: An uninterpretable body starts nothing

T6.3 IF the extension cannot interpret the body, THEN the extension SHALL start no turn.
T6.3 IF the extension cannot interpret the body, THEN the extension SHALL publish no `received` acknowledgement.

#### Scenario: An uninterpretable body is not acknowledged
- **WHEN** the extension cannot interpret a record body and the sender asked for acknowledgements
- **THEN** the extension SHALL publish no `received` acknowledgement
