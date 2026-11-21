# Spec Delta

## ADDED Requirements

### Requirement: Attachment is a state the extension rebuilds

The extension SHALL treat its bus subscription as state that is rebuilt, not as an event that
either succeeds once or ends the session's ability to consume. A failed read, a lost wake, or a
closed subscription SHALL leave the persisted offset untouched and the intent to consume intact,
and the extension SHALL retry the same address with the same consumer identity.

#### Scenario: A failed read retries with the same cursor
- **WHEN** reading from the bus throws while the session is running
- **THEN** the extension SHALL retry that address from the offset it had already persisted
- **AND** the session SHALL continue its turn without waiting for the retry

#### Scenario: Retry is bounded and backed off
- **WHEN** consecutive reads keep failing
- **THEN** the delay between attempts SHALL grow to a cap rather than spin
- **AND** the extension SHALL keep retrying for as long as the session lives

#### Scenario: A lost subscription is rebuilt at a turn boundary
- **WHEN** the session settles a turn while any subscribed address is not attached
- **THEN** the extension SHALL attempt to attach that address before the turn ends

#### Scenario: A restarted session attaches again
- **WHEN** a session starts from a saved transcript
- **THEN** the extension SHALL attach to every configured address from the offset already stored for its consumer
- **AND** it SHALL do so whether or not the previous process shut down cleanly

#### Scenario: A failed attach skips no record
- **WHEN** an attach attempt fails before any record is read
- **THEN** the persisted offset SHALL be unchanged
- **AND** the records that arrive in the meantime SHALL be delivered after a later attempt

### Requirement: A replayed backlog is announced

When the extension attaches to an address whose persisted offset is behind the newest record, it
SHALL deliver from that offset, not from the end of the stream, and SHALL report how many records
were waiting before delivering them.

#### Scenario: The backlog is counted before it is read
- **WHEN** the extension attaches with a saved offset behind the head of the address
- **THEN** the session SHALL be told how many records were waiting

#### Scenario: An attached extension says nothing extra
- **WHEN** the extension attaches with a saved offset at the head of the address
- **THEN** no replay announcement SHALL be produced

#### Scenario: Replay neither duplicates nor drops
- **WHEN** a backlog is replayed after a failure
- **THEN** every record after the saved offset SHALL be delivered exactly once

### Requirement: Loss and recovery are visible to the session

A delivery failure and the recovery from it SHALL appear in the session's transcript as msgq
records, with the reason and the affected address, and SHALL NOT exist only as a transient UI
notification. The session's status SHALL state whether it is attached, detached and retrying, or
disabled.

#### Scenario: A detachment is readable by the model
- **WHEN** the extension cannot read from an address
- **THEN** a msgq record in the transcript SHALL name the address and the failure
- **AND** the session SHALL be able to tell that it cannot receive

#### Scenario: Recovery precedes the backlog
- **WHEN** a detached extension attaches again
- **THEN** the transcript SHALL report the recovery before the replayed records

#### Scenario: The status never claims attachment that failed
- **WHEN** no address is attached
- **THEN** the status slot SHALL say so rather than report the configured subscription count

#### Scenario: A UI-less session still records the failure
- **WHEN** the extension cannot read from an address in a session without a UI
- **THEN** the transcript SHALL still carry the failure and the recovery

### Requirement: A session that cannot receive does not claim to be reachable

While the extension is detached it SHALL stop renewing its presence lease, and it SHALL publish
presence again only after it is attached. A roster entry promises delivery, so a session that
cannot consume SHALL NOT keep advertising itself as available.

#### Scenario: Detachment stops the renewal
- **WHEN** the extension is detached from every configured address
- **THEN** it SHALL stop renewing its presence lease
- **AND** its roster entry SHALL expire instead of staying current

#### Scenario: Recovery republishes presence
- **WHEN** a detached extension attaches again
- **THEN** it SHALL publish its presence in the current state before the next renewal interval

#### Scenario: A peer does not address a detached session by discovery
- **WHEN** a peer lists the roster after a detached session's lease has expired
- **THEN** that session SHALL NOT be offered as available

## MODIFIED Requirements

### Requirement: Loading and lifecycle
The extension SHALL be loadable ad hoc from a directory with `pi -e <path>` without being
installed or published. It SHALL begin its work on the session start event and SHALL NOT start
long-lived resources when its factory runs. It SHALL release all resources on session shutdown,
and cleanup SHALL be idempotent across reload, session replacement, and process exit. A release
caused by a failed subscription SHALL end only that subscription, not the session's ability to
consume.

#### Scenario: Loaded from a directory
- **WHEN** pi starts with `-e ./extension`
- **THEN** the extension loads and starts consuming without being installed

#### Scenario: No work before a session starts
- **WHEN** the extension factory runs
- **THEN** no subscription, timer, or engine handle is created

#### Scenario: Reload is clean
- **WHEN** the session starts with reason `reload`
- **THEN** any previous subscription and engine handle are released and exactly one new subscription per address is active

#### Scenario: Idempotent shutdown
- **WHEN** shutdown runs more than once for the same runtime
- **THEN** releasing resources a second time has no effect and raises no error

#### Scenario: A broken subscription is not a shutdown
- **WHEN** a subscription fails and is rebuilt
- **THEN** the bus handle, the presence clock, and the other subscriptions SHALL stay as they were

### Requirement: Graceful degradation

T6.4 IF the bus is unreadable, THEN the extension SHALL retry and report the detachment.
T6.4 IF the configuration is unusable, THEN the extension SHALL report the extension disabled.
T6.4 IF the credential is rejected, THEN the extension SHALL report that the session cannot write.
T6.4 IF a record carries an unsupported version, THEN the extension SHALL skip the record.
T6.4 IF a session cannot write, THEN the reported state SHALL name the credential problem.

#### Scenario: Unreadable bus
- **WHEN** the bus cannot be opened
- **THEN** the extension SHALL retry and SHALL keep the session running

#### Scenario: A failed subscription is rebuilt
- **WHEN** a subscription fails mid-session
- **THEN** the extension SHALL rebuild it from the saved offset instead of disabling itself

#### Scenario: Unusable configuration disables
- **WHEN** the configuration names an unknown policy or an unknown address scheme
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
