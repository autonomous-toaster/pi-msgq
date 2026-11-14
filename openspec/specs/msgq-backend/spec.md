# msgq-backend Specification

## Purpose
The storage contract every engine behind the bus satisfies, so that one product runs on a local file
or on a shared server without changing what the bus promises to the clients above the engine.

## Requirements

### Requirement: Backend selection

T3.2 IF the resolved location is a server connection string, THEN the bus SHALL select the shared engine.
T3.2 IF the resolved location is a filesystem path, THEN the bus SHALL select the local engine.
T3.2 IF the resolved location names an unimplemented scheme, THEN the bus SHALL refuse the location.
T3.2 IF the resolved location names an unimplemented scheme, THEN the bus SHALL write nothing.

#### Scenario: Path selects the local engine
- **WHEN** the resolved location is a filesystem path
- **THEN** the bus SHALL use the local file engine

#### Scenario: Connection string selects the shared engine
- **WHEN** the resolved location is a server connection string
- **THEN** the bus SHALL use the shared engine

#### Scenario: No silent fallback
- **WHEN** the resolved location names an engine the build does not support
- **THEN** the command SHALL fail with an error naming the found location

#### Scenario: Two engines never exchange records
- **WHEN** a local bus and a shared bus both run
- **THEN** neither bus SHALL deliver to the other

### Requirement: One engine per deployment

T4.1 ALWAYS, exactly one engine SHALL serve one deployment.
T4.1 ALWAYS, no relay SHALL exist between engines.
T4.1 ALWAYS, no synchronization SHALL copy records between engines.
T2.8 ALWAYS, the documentation SHALL name the two engines and the location form that selects each.

#### Scenario: One engine serves a deployment
- **WHEN** a deployment starts
- **THEN** exactly one engine SHALL serve the deployment

#### Scenario: No message crosses deployments
- **WHEN** two deployments run on different engines
- **THEN** the bus SHALL confine every record to the deployment that wrote the record

### Requirement: One address maps to one stream

T4.2 ALWAYS, one canonical address SHALL resolve to exactly one stream.
T4.2 ALWAYS, two addresses differing only by letter case SHALL resolve to two distinct streams.
T4.3 ALWAYS, one address SHALL resolve to the same stream name for the life of the bus.

#### Scenario: Distinct addresses stay distinct
- **WHEN** two addresses differ only by letter case
- **THEN** the two addresses SHALL resolve to two distinct streams

#### Scenario: The mapping is stable
- **WHEN** the same address is published to on two separate days
- **THEN** the bus SHALL write both records to the same stream

### Requirement: A long address still maps

T4.2 IF an address exceeds the engine's stream-name limit, THEN the bus SHALL derive a stream name within the limit.
T4.2 IF two addresses derive the same readable portion, THEN the bus SHALL derive two different stream names.

#### Scenario: A long address still maps
- **WHEN** an address is longer than the engine's stream-name limit
- **THEN** the bus SHALL publish to one stream for that address

#### Scenario: An address survives a naming collision
- **WHEN** two addresses would derive the same human-readable portion of a stream name
- **THEN** the bus SHALL derive two different stream names

### Requirement: The address stays recoverable

T4.2 ALWAYS, the bus SHALL keep the original address recoverable for every stream that carries records.
T4.2 ALWAYS, the bus SHALL keep the original address recoverable for every registry entry.

#### Scenario: Case is preserved
- **WHEN** both `session/Api` and `session/api` are used on an engine that lowercases identifiers
- **THEN** the bus SHALL keep the two addresses distinct

#### Scenario: An address is readable from the bus
- **WHEN** a registry entry exists for an address
- **THEN** the bus SHALL report the original address for the registry entry

### Requirement: Streams are created on first publish

T4.3 BEFORE the bus writes a first record to an address, the bus SHALL create the address's stream.
T4.3 BEFORE the bus reads an unpublished address, the bus SHALL create no stream.

#### Scenario: First publish creates
- **WHEN** a record is published to an address that has no stream yet
- **THEN** the bus SHALL write the record

#### Scenario: Reading does not create
- **WHEN** an address that was never published to is read
- **THEN** the bus SHALL report no records

### Requirement: Concurrent first publishes

T4.3 CONCURRENTLY, two processes publishing to one new address SHALL both succeed.
T4.3 CONCURRENTLY, two processes creating one stream SHALL produce one stream.

#### Scenario: Concurrent first publishes
- **WHEN** two processes publish to the same new address at the same time
- **THEN** the bus SHALL write both records

#### Scenario: Creation is idempotent
- **WHEN** two processes create the same stream at the same time
- **THEN** the bus SHALL keep one stream

### Requirement: A read never moves a cursor

T4.5 ALWAYS, a read SHALL leave every cursor unchanged.
T4.5 ALWAYS, a read SHALL hide no record from another consumer.
T4.4 ALWAYS, two consumers SHALL each receive the same record under two separate cursors.

#### Scenario: Reading twice yields the same records
- **WHEN** two reads use the same starting position
- **THEN** both reads SHALL return the same records

#### Scenario: Two consumers both see a record
- **WHEN** two consumers read the same address
- **THEN** each consumer SHALL receive the same record under a separate cursor

### Requirement: A cursor never goes backwards

T4.5 IF a cursor write carries a lower value than the stored value, THEN the bus SHALL ignore the write.

#### Scenario: A cursor does not go backwards
- **WHEN** a cursor is written with a lower value than the stored value
- **THEN** the bus SHALL keep the stored value

### Requirement: History reads are read-only

T4.4 ALWAYS, a history read SHALL leave every counter unchanged.
T4.4 ALWAYS, a history read SHALL leave every visibility marker unchanged.
T4.4 ALWAYS, the bus SHALL return identical records for two identical reads.

#### Scenario: A history read is idempotent
- **WHEN** an address is read from the beginning twice
- **THEN** both reads SHALL report identical records

#### Scenario: The newest records can be inspected without consuming
- **WHEN** a caller requests the last N records of an address
- **THEN** the bus SHALL return the records in order

### Requirement: A waiter is woken by the bus

T4.6 IF the bus writes a record to a followed address, THEN the bus SHALL wake the waiting consumer.
T4.6 IF the engine cannot push a wake, THEN the consumer SHALL fall back to polling.

#### Scenario: A write wakes a waiter
- **WHEN** a record is written while a consumer waits on the address
- **THEN** the consumer SHALL return before a fixed interval elapses

#### Scenario: Push unavailable, polling still works
- **WHEN** the engine cannot deliver a wake on the active connection
- **THEN** the consumer SHALL still receive the record through polling

### Requirement: Retention removes old records

T4.7 AFTER a record exceeds the retention window, the bus SHALL remove the record.
T4.7 AFTER a removal, the bus SHALL report the number of removed records.
T4.7 AFTER the bus removes records, the bus SHALL keep the newest presence record of each participant.

#### Scenario: Old records go
- **WHEN** records are older than the retention window
- **THEN** the bus SHALL remove the records

#### Scenario: Newest presence survives
- **WHEN** every presence record of a participant is older than the retention window
- **THEN** the bus SHALL keep the most recent presence record

### Requirement: Removal is bounded

T2.3 IF a prefix exceeds one batch, THEN the bus SHALL delete the prefix in bounded batches.

#### Scenario: Removal is bounded
- **WHEN** a large prefix is older than the retention window
- **THEN** the bus SHALL remove the prefix in batches

### Requirement: Both engines pass the same fixtures

T3.4 ALWAYS, every shipped engine SHALL pass one behavioural fixture set.
T4.9 ALWAYS, two engines SHALL produce the same outcome for one fixture.
T7.1 ALWAYS, continuous integration SHALL run the conformance suites against each engine.

#### Scenario: Fixtures agree across engines
- **WHEN** the behavioural fixtures run against each engine
- **THEN** both engines SHALL produce the same outcome for every fixture

#### Scenario: Drift is caught
- **WHEN** one engine changes the cursor behaviour and the other engine does not
- **THEN** a conformance test SHALL fail

### Requirement: Every operation passes through a scoped function

T2.2 ALWAYS, an operation on a shared engine SHALL pass through a function that takes the credential.
T2.2 ALWAYS, the function SHALL resolve the credential at the start of the call.
T2.2 ALWAYS, the function SHALL check the address against the scopes of the resolved principal.

#### Scenario: The read function refuses another tenant
- **WHEN** a client calls the read function for an address outside the scopes of the client
- **THEN** the function SHALL refuse the call

### Requirement: The runtime role holds no direct access

T3.1 ALWAYS, the runtime role SHALL hold no privilege on a queue table.
T3.1 ALWAYS, the runtime role SHALL hold no privilege to create a queue.
T3.1 ALWAYS, the runtime role SHALL hold execute privilege on the function surface only.

#### Scenario: A direct read is refused
- **WHEN** the runtime role reads a queue table directly
- **THEN** the engine SHALL refuse the read

#### Scenario: A direct delete is refused
- **WHEN** the runtime role deletes from a queue table directly
- **THEN** the engine SHALL refuse the delete

### Requirement: Prune owns the watermark

T3.2 IF a prune removes records, THEN the bus SHALL write the retention watermark for that address.
T3.2 IF a caller writes a retention watermark without a prune, THEN the bus SHALL refuse the write.
T3.2 IF a prune names an address outside the scopes of the caller, THEN the bus SHALL refuse the prune.

#### Scenario: A forged watermark is refused
- **WHEN** a caller writes a retention watermark without removing records
- **THEN** the bus SHALL refuse the write

#### Scenario: A scoped prune removes only the caller's records
- **WHEN** a caller prunes an address inside the scopes of the caller
- **THEN** the prune SHALL remove records from that address alone

### Requirement: A cursor is owned

T3.3 ALWAYS, the bus SHALL stamp the principal of the caller as the owner of a cursor row.
T3.3 ALWAYS, a cursor operation SHALL address a cursor row owned by the caller.

#### Scenario: One consumer name in two tenants
- **WHEN** two principals each save a cursor under one consumer name
- **THEN** the bus SHALL store two cursor rows owned by the two principals
