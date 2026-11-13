# Spec Delta

## MODIFIED Requirements

### Requirement: Storage location and resolution

T3.2 ALWAYS, the CLI SHALL prefer the `--db` argument over every environment variable.
T3.2 ALWAYS, the CLI SHALL prefer `MSGQ_DB_URI` over `MSGQ_DB`.
T3.2 ALWAYS, the CLI SHALL prefer `MSGQ_DB` over the default path.
T3.2 ALWAYS, the CLI SHALL keep a connection string in `MSGQ_DB` working.
T3.2 ALWAYS, the CLI SHALL read the server settings `MSGQ_PGHOST`, `MSGQ_PGPORT`, `MSGQ_PGUSER`, `MSGQ_PGPASSWORD`, `MSGQ_PGDATABASE` and `MSGQ_PGSSLMODE`.
T3.2 ALWAYS, the CLI SHALL use `~/.pi/agent/msgq/msgq.db` as the default location.
T3.2 ALWAYS, a server connection string SHALL select the shared backend.
T3.2 ALWAYS, a filesystem path SHALL select the local backend.
T3.2 ALWAYS, an unimplemented scheme SHALL cause a usage error.
T3.2 ALWAYS, the CLI SHALL create a missing local bus file.
T7.2 ALWAYS, the documentation SHALL name the location form of each backend.

#### Scenario: Default path
- **WHEN** `--db`, `MSGQ_DB_URI`, `MSGQ_DB` and the server settings are absent
- **THEN** the CLI SHALL use `~/.pi/agent/msgq/msgq.db`

#### Scenario: Environment override
- **WHEN** `MSGQ_DB` is set and `--db` is absent
- **THEN** the CLI SHALL use the `MSGQ_DB` path

#### Scenario: Server wins over file
- **WHEN** `MSGQ_DB_URI` and `MSGQ_DB` are set and `--db` is absent
- **THEN** the CLI SHALL use the `MSGQ_DB_URI` connection string

#### Scenario: Flag precedence
- **WHEN** `--db` and `MSGQ_DB` are provided
- **THEN** the CLI SHALL use the `--db` location

#### Scenario: First use creates the database
- **WHEN** the resolved database file does not exist
- **THEN** the CLI SHALL create the file

#### Scenario: Connection string selects the shared backend
- **WHEN** the resolved location is a server connection string
- **THEN** every command in that invocation SHALL use the shared backend

#### Scenario: Unsupported scheme is refused
- **WHEN** the resolved location names a scheme the CLI does not implement
- **THEN** the CLI SHALL exit with a usage error

### Requirement: Record identity

T1.1 ALWAYS, a record id SHALL name the address and the offset together as `<address>#<offset>`.
T1.1 ALWAYS, offsets SHALL be unique within a stream.
T1.1 ALWAYS, the CLI SHALL include the address in every record name.
T1.1 ALWAYS, the CLI SHALL derive the id from the record.
T1.1 ALWAYS, the CLI SHALL store no id field in the record.
T1.1 ALWAYS, `reply_to` SHALL hold an offset that the `from` field of the answering record addresses.
T7.2 ALWAYS, the documentation SHALL state that an offset alone does not name a record.
T2.7 ALWAYS, a record id SHALL name the record being awaited.
T2.7 ALWAYS, the CLI SHALL refuse a bare offset for a wait.

#### Scenario: Derived, not stored
- **WHEN** a record is published
- **THEN** the record id SHALL name the published address and the assigned offset

#### Scenario: A reference identifies its record
- **WHEN** a published record carries `reply_to`
- **THEN** the CLI SHALL resolve the referred record in the stream named by the referring record's `from`

#### Scenario: A reference is exact rather than inferred
- **WHEN** a reply is published to the address of the answering session
- **THEN** `reply_to` SHALL name the offset in the stream of the reply's `from`

#### Scenario: Earlier records have ids too
- **WHEN** a caller reads a record published earlier
- **THEN** the CLI SHALL form an id from the address and the offset

#### Scenario: Equal offsets in different streams
- **WHEN** two addresses each hold a record at offset 7
- **THEN** the CLI SHALL name the two records `group/a#7` and `session/b#7`

### Requirement: Stateless read

T2.4 ALWAYS, a stateless read SHALL create no cursor.
T2.4 ALWAYS, a stateless read SHALL advance no cursor.
T2.4 ALWAYS, a stateless read SHALL cover the retained window.
T2.4 ALWAYS, `--from-start` SHALL mean the oldest retained record.
T2.4 ALWAYS, the CLI SHALL report the number of records lost to the window.
T2.4 ALWAYS, `--strict` SHALL cause a non-zero exit for a truncated read.

#### Scenario: Default start position
- **WHEN** a caller invokes read without start flags
- **THEN** the CLI SHALL read from the current end of each stream

#### Scenario: Read from the start
- **WHEN** `--from-start` is provided
- **THEN** the CLI SHALL read from the oldest retained record

#### Scenario: Read from an offset
- **WHEN** `--from N` is provided
- **THEN** the CLI SHALL return messages with an offset greater than `N`

#### Scenario: Repeated read does not consume
- **WHEN** a caller invokes read twice with the same arguments
- **THEN** the CLI SHALL return the same messages on both invocations

#### Scenario: Truncated history is reported
- **WHEN** `--from N` names an offset older than the retained window
- **THEN** the CLI SHALL report the number of missing records

#### Scenario: A gap is fatal only on request
- **WHEN** history was truncated and `--strict` is present
- **THEN** the CLI SHALL exit non-zero

### Requirement: Cursor-based follow

T2.4 IF a consumer has a saved offset, THEN the follow SHALL resume from the saved offset.
T2.4 IF a consumer has no saved offset, THEN the follow SHALL start at the current end.
T2.4 IF the caller passes `--from-start`, THEN the follow SHALL start at the oldest retained record.
T2.4 IF a consumer offset is older than the retained window, THEN the follow SHALL report the lost count.
T2.4 IF a consumer offset is older than the retained window, THEN the follow SHALL resume at the oldest retained record.

#### Scenario: Interruption is safe
- **WHEN** a caller interrupts a follow with SIGINT
- **THEN** the CLI SHALL save the cursor

#### Scenario: New consumer does not replay
- **WHEN** a consumer follows a stream for the first time without start flags
- **THEN** the CLI SHALL replay no previously published message

#### Scenario: Explicit replay
- **WHEN** `--from-start` is provided
- **THEN** the CLI SHALL deliver the retained history

#### Scenario: A consumer behind the window is told
- **WHEN** a consumer offset is older than the oldest retained record
- **THEN** the follow SHALL report the number of lost records

#### Scenario: A gap does not stop delivery
- **WHEN** a follow has reported a gap
- **THEN** the follow SHALL keep delivering subsequent records

### Requirement: Topics listing
T4.8 ALWAYS, `msgq topics` SHALL list each address with the message count and the highest offset.
T4.8 ALWAYS, `msgq topics` SHALL list a registered address with a count of zero.

#### Scenario: Distinct addresses
- **WHEN** messages exist for `group/a` and `session/b`
- **THEN** the listing SHALL show both addresses exactly once

#### Scenario: No messages
- **WHEN** no messages have been published
- **THEN** the command SHALL succeed

#### Scenario: An empty but known address is listed
- **WHEN** an address has a registry entry and every record of the address aged out
- **THEN** the listing SHALL show the address with a count of zero

### Requirement: Consumers and cursors

T4.8 ALWAYS, `msgq consumers` SHALL list each consumer with the topic and the offset.
T2.4 ALWAYS, `msgq consumers` SHALL report the lag of each consumer.
T2.4 ALWAYS, `msgq consumers` SHALL report the lost count of each consumer.
T4.8 ALWAYS, `msgq consumers rm` SHALL remove matching cursors.
T4.8 ALWAYS, `msgq cursor get` SHALL read a cursor.
T4.8 ALWAYS, `msgq cursor set` SHALL set a cursor.
T4.8 ALWAYS, `msgq cursor reset` SHALL clear a cursor.

#### Scenario: Monotonic cursor
- **WHEN** a caller sets a cursor to a lower offset than the stored offset
- **THEN** the stored offset SHALL NOT decrease

#### Scenario: Consumer removal
- **WHEN** a caller removes a consumer
- **THEN** the listing SHALL drop the consumer

#### Scenario: Lag is visible
- **WHEN** a consumer has not read the newest records of the stream
- **THEN** the listing SHALL show the lag

#### Scenario: Lost records are visible
- **WHEN** a consumer offset is older than the oldest retained record
- **THEN** the listing SHALL show the lost count

### Requirement: Database info and protocol metadata

T2.5 ALWAYS, `msgq db info` SHALL report the backend.
T2.5 ALWAYS, `msgq db info` SHALL report the retention window.
T2.5 ALWAYS, `msgq db info` SHALL report the per-stream message counts.
T2.5 ALWAYS, `msgq db info` SHALL report the consumer counts.
T2.5 ALWAYS, `msgq db info` SHALL report the protocol version.
T2.5 ALWAYS, `msgq db info` SHALL report the lag of each consumer.
T2.5 ALWAYS, `msgq db info` SHALL report the oldest-record age of each stream.
T2.5 ALWAYS, `msgq db info` SHALL report the presence address age separately.
T2.5 ALWAYS, `msgq db info` SHALL report the resolved location.
T5.4 ALWAYS, `msgq db info` SHALL report the credential state.
T2.5 ALWAYS, the bus SHALL record `protocol_version` and `created_by` in a bus table.
T2.5 ALWAYS, the bus SHALL modify no engine table on the write path.

#### Scenario: New database records protocol version
- **WHEN** the CLI creates the database
- **THEN** the bus SHALL record protocol version 1

#### Scenario: Info reports provenance
- **WHEN** a caller runs `msgq db info`
- **THEN** the command SHALL report the protocol version

#### Scenario: Info answers whether anything is stuck
- **WHEN** a caller runs `msgq db info` on a bus in use
- **THEN** the command SHALL report the lag of each consumer

#### Scenario: Info names the backend
- **WHEN** a caller runs `msgq db info` against either backend
- **THEN** the command SHALL name the backend

#### Scenario: Info reports write authority
- **WHEN** a caller runs `msgq db info` against an authorizing backend
- **THEN** the command SHALL report the credential state

### Requirement: Concurrency safety

T3.1 AT MOST ONE process SHALL hold the follow lock for one consumer and address pair.

#### Scenario: Duplicate follow rejected
- **WHEN** a second follow starts for the same consumer and address
- **THEN** the second follow SHALL exit with an error

#### Scenario: Concurrent publish and follow
- **WHEN** a publisher and a follower run together
- **THEN** both processes SHALL succeed

#### Scenario: The lock follows the backend
- **WHEN** the active backend changes
- **THEN** the follow lock SHALL come from the active backend

### Requirement: Awaiting acknowledgements

T1.1 IF the caller gives a bare offset without an address, THEN the CLI SHALL exit with a usage error.
T1.1 IF the caller gives a record id or `--on`, THEN the wait SHALL follow the named address.
T1.1 IF `msgq send --await` runs, THEN the CLI SHALL publish the message first.
T1.1 IF the CLI publishes an awaited message, THEN the CLI SHALL record the starting position.
T1.1 IF an acknowledgement arrives, THEN the CLI SHALL report the phase.
T1.1 IF a terminal acknowledgement arrives, THEN the CLI SHALL finish.
T1.1 IF the caller passes `--json`, THEN the CLI SHALL emit one JSON object per acknowledgement.

#### Scenario: Phases are reported as they arrive
- **WHEN** an awaited message is started and then completed
- **THEN** the CLI SHALL report each phase

#### Scenario: An early acknowledgement is not missed
- **WHEN** an acknowledgement arrives between publishing and the wait
- **THEN** the CLI SHALL report the acknowledgement

#### Scenario: Reply address is derived
- **WHEN** the record id names an address, or `--on` is given
- **THEN** the wait SHALL follow the named address

#### Scenario: A bare offset is refused
- **WHEN** a caller runs `msgq await 42` without an address
- **THEN** the CLI SHALL exit with a usage error

#### Scenario: Terminal phase ends the wait
- **WHEN** a terminal acknowledgement arrives
- **THEN** the command SHALL finish successfully

#### Scenario: Display delivery ends the wait
- **WHEN** the awaited message reached display-only delivery
- **THEN** the wait SHALL finish at the `received` acknowledgement

#### Scenario: Timeout
- **WHEN** no terminal acknowledgement arrives within the wait window
- **THEN** the CLI SHALL report the timeout

#### Scenario: Unbounded wait
- **WHEN** the wait window is zero
- **THEN** the CLI SHALL wait without a limit

#### Scenario: JSON output
- **WHEN** `msgq await --json` is given
- **THEN** the CLI SHALL emit each acknowledgement as one JSON object per line

## ADDED Requirements

### Requirement: A follow takes the backend lock

T3.1 IF a follow starts, THEN the CLI SHALL take the backend lock.
T3.1 IF another process holds the lock, THEN the CLI SHALL fail loudly.
T3.1 IF the local backend serves the session, THEN the CLI SHALL rely on WAL mode and a bounded busy timeout.
T3.1 IF the shared backend serves the session, THEN the CLI SHALL rely on engine isolation.

#### Scenario: The lock is taken on start
- **WHEN** a follow starts
- **THEN** the CLI SHALL take the backend lock

#### Scenario: The lock is refused
- **WHEN** another process holds the lock
- **THEN** the CLI SHALL fail loudly

### Requirement: The offset is saved first

T2.4 BEFORE the CLI reports a message, the CLI SHALL persist the consumer offset.

#### Scenario: Save before report
- **WHEN** the CLI delivers a message to a following caller
- **THEN** the CLI SHALL persist the offset first

### Requirement: Retention is a declared window

T2.1 ALWAYS, `msgq prune` SHALL remove records older than the retention window.
T2.1 ALWAYS, `msgq prune` SHALL ignore consumer cursors when selecting records.
T2.1 ALWAYS, the retention window SHALL default to one hour.
T2.1 ALWAYS, the CLI SHALL accept the retention window from the environment.
T2.1 ALWAYS, the CLI SHALL accept the retention window from the command line.

#### Scenario: Old records go
- **WHEN** records are older than the retention window and the caller passes `--apply`
- **THEN** the command SHALL remove the records

#### Scenario: A lagging consumer does not pin history
- **WHEN** a consumer cursor sits before a record older than the retention window
- **THEN** the command SHALL remove the record

#### Scenario: The window is configurable
- **WHEN** the caller gives a retention window in the environment or on the command line
- **THEN** the command SHALL use that window

### Requirement: Pruning is reported

T2.2 IF the caller supplies no `--apply` flag, THEN `msgq prune` SHALL report the candidate records.
T2.2 IF the caller supplies no `--apply` flag, THEN `msgq prune` SHALL remove nothing.
T2.3 IF a prefix exceeds one batch, THEN `msgq prune` SHALL delete the prefix in bounded batches.
T4.7 IF a consumer loses records, THEN `msgq prune` SHALL name the consumer.
T4.7 IF every presence record of a participant is older than the window, THEN `msgq prune` SHALL keep the newest presence record.

#### Scenario: Nothing is removed unasked
- **WHEN** a caller runs `msgq prune` without `--apply`
- **THEN** the command SHALL report the candidates

#### Scenario: Removal is batched
- **WHEN** a large prefix is older than the retention window
- **THEN** the command SHALL remove the prefix in bounded batches

#### Scenario: Presence survives
- **WHEN** every presence record of a participant is older than the retention window
- **THEN** the command SHALL keep the newest presence record of the participant

### Requirement: Publishing under an authorizing backend

T5.4 IF the active backend can authorize, THEN the CLI SHALL present a credential.
T5.4 IF the active backend can authorize, THEN the bus SHALL store the credential address as the sender.
T5.4 IF the bus refuses a write, THEN the CLI SHALL exit non-zero.
T5.4 IF the bus refuses a write, THEN the CLI SHALL write nothing.
T5.4 IF the bus refuses a write, THEN the CLI SHALL NOT retry without a credential.
T5.4 IF the active backend cannot authorize, THEN the CLI SHALL require no credential.

#### Scenario: The sender comes from the credential
- **WHEN** a caller publishes with a sender other than the credential address
- **THEN** the stored record SHALL carry the credential address

#### Scenario: Refusal is an error
- **WHEN** the credential is missing, expired, or out of scope for the address
- **THEN** the publish SHALL exit non-zero

#### Scenario: No silent fallback
- **WHEN** the bus refuses a publish under authorization
- **THEN** the CLI SHALL NOT retry without a credential

#### Scenario: The local backend is unchanged
- **WHEN** the active backend cannot authorize writes
- **THEN** the CLI SHALL require no credential

## REMOVED Requirements

### Requirement: Retention
**Reason**: T2.1 ALWAYS, a record older than the retention window SHALL leave the bus. The previous rule held history indefinitely whenever one consumer lagged.
**Migration**: T2.1 ALWAYS, `msgq prune --older-than` SHALL apply the window. `msgq consumers` SHALL report how many records a consumer can no longer read.

### Requirement: Server settings complete the location

T3.2 IF no location variable names a bus and `MSGQ_PGHOST` is set, THEN the CLI SHALL select the shared backend.
T3.2 IF no location variable names a bus and `MSGQ_PGDATABASE` is set, THEN the CLI SHALL select the shared backend.
T3.2 IF the location does not name a host, THEN the CLI SHALL take the host from `MSGQ_PGHOST`.
T3.2 IF the location does not name a password, THEN the CLI SHALL take the password from `MSGQ_PGPASSWORD`.
T3.2 IF `MSGQ_PGSSLMODE` demands TLS, THEN the CLI SHALL refuse the setting with a usage error.

#### Scenario: Settings name a server
- **WHEN** `MSGQ_PGHOST` is set and every location variable is absent
- **THEN** the CLI SHALL select the shared backend

#### Scenario: A secret stays out of the location
- **WHEN** the location names a host and `MSGQ_PGPASSWORD` is set
- **THEN** the CLI SHALL connect with that password

#### Scenario: TLS is refused rather than ignored
- **WHEN** `MSGQ_PGSSLMODE` is `require`
- **THEN** the CLI SHALL exit with a usage error
