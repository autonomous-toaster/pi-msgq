# msgq-cli Specification

## Purpose

`msgq` is a durable, topic-addressed message bus over a single local SQLite file, letting scripts, humans, and (later) pi sessions publish and consume messages without a daemon, broker, or network service.

## Requirements

### Requirement: Storage location and resolution
The CLI SHALL resolve the database path in this order: the `--db` argument, then the `MSGQ_DB` environment variable, then `~/.pi/agent/msgq/msgq.db`. The CLI SHALL create and initialize the database on first use when the resolved file does not exist.

#### Scenario: Default path
- **WHEN** neither `--db` nor `MSGQ_DB` is provided
- **THEN** the CLI uses `~/.pi/agent/msgq/msgq.db`

#### Scenario: Environment override
- **WHEN** `MSGQ_DB` is set and `--db` is not provided
- **THEN** the CLI uses the `MSGQ_DB` path

#### Scenario: Flag precedence
- **WHEN** both `--db` and `MSGQ_DB` are provided
- **THEN** the `--db` path is used

#### Scenario: First use creates the database
- **WHEN** the resolved database file does not exist
- **THEN** the CLI creates it and initializes the schema, and the command proceeds

### Requirement: Address grammar and canonicalization
An address SHALL identify both a recipient and the stream it is written to. When an address contains `/`, its prefix MUST be one of `session`, `cwd`, `group`, or `cli`; any other prefix is a usage error. An address without `/` SHALL be canonicalized to `group/<name>`. Addresses SHALL be case-sensitive with surrounding whitespace trimmed. An empty address, or a scheme-only address such as `session/`, SHALL be rejected.

#### Scenario: Bare name canonicalization
- **WHEN** a message is published to `build.done`
- **THEN** the stream `group/build.done` is written and `topics` lists `group/build.done`

#### Scenario: Unknown prefix rejected
- **WHEN** an address such as `foo/bar` is used
- **THEN** the CLI exits with a usage error and writes nothing

#### Scenario: Scheme-only address rejected
- **WHEN** an address such as `session/` is used
- **THEN** the CLI exits with a usage error

#### Scenario: Case sensitivity
- **WHEN** `session/Api` and `session/api` are used
- **THEN** they address two distinct streams

### Requirement: Message record contract
A published message SHALL be a JSON object carrying `v`, `type`, `from`, and `to`, and for message-bearing types a `message`. `v` MUST be `1`. `type` SHALL be one of `prompt`, `steer`, `follow_up`, `note`, or `event`, and consumers MUST tolerate unknown types. `images`, `reply_to`, and `meta` SHALL be optional. When `type` is `event`, an `event` object MUST be present in place of `message`. A record SHALL NOT carry a publisher-supplied timestamp field.

#### Scenario: Shorthand record
- **WHEN** a message is published in shorthand text mode
- **THEN** the stored record contains `v`, `type`, `from`, `to`, and `message`

#### Scenario: Unknown type tolerated
- **WHEN** a consumer reads a record whose `type` it does not recognize
- **THEN** it displays the record without error

#### Scenario: Event record
- **WHEN** `type` is `event`
- **THEN** the record carries an `event` object and is accepted by the bus

### Requirement: Payload passthrough
When a complete record is supplied through `--json` or on stdin, the CLI SHALL store it without adding, removing, or rewriting any field of the record body.

#### Scenario: Unknown fields preserved
- **WHEN** a supplied record contains fields the CLI does not recognize
- **THEN** those fields are preserved in the stored record

### Requirement: Publish
`msgq publish <address> [message]` SHALL append one message to the stream named by the canonical address and print the assigned offset to stdout.

#### Scenario: Successful publish
- **WHEN** a message is published
- **THEN** the assigned offset is printed and the message is returned by a subsequent read or follow

#### Scenario: Default sender
- **WHEN** `--from` is omitted
- **THEN** the record's `from` is `cli/<user>@<host>`

#### Scenario: Body from stdin
- **WHEN** the message argument is `-`
- **THEN** the body is read from stdin

### Requirement: Stateless read
`msgq read <address>...` SHALL return messages from the named streams without creating or advancing any consumer cursor.

#### Scenario: Default start position
- **WHEN** read is invoked without start flags
- **THEN** it reads from the current end of each stream and does not replay history

#### Scenario: Read from the start
- **WHEN** `--from-start` is provided
- **THEN** it reads from the earliest message

#### Scenario: Read from an offset
- **WHEN** `--from N` is provided
- **THEN** it returns messages whose offset is greater than `N`

#### Scenario: Repeated read does not consume
- **WHEN** read is invoked twice with the same arguments
- **THEN** the second invocation still returns the same messages because no cursor was advanced

### Requirement: Cursor-based follow
`msgq follow <address>... --consumer <name>` SHALL resume from the consumer's saved offset and advance that offset as messages are delivered. A consumer with no saved offset SHALL start at the current end of the stream unless `--from-start` or `--from N` is provided. The consumer offset SHALL be persisted before each message is reported, so an interruption replays at most one message.

#### Scenario: Interruption is safe
- **WHEN** a follow is interrupted by SIGINT
- **THEN** the cursor is saved and the CLI exits with status 0

#### Scenario: New consumer does not replay
- **WHEN** a consumer follows a stream for the first time without start flags
- **THEN** previously published messages are not replayed

#### Scenario: Explicit replay
- **WHEN** `--from-start` is provided
- **THEN** the full history is delivered and the cursor advances

### Requirement: Topics listing
`msgq topics` SHALL list each distinct address with its message count and highest offset.

#### Scenario: Distinct addresses
- **WHEN** messages exist for `group/a` and `session/b`
- **THEN** both addresses appear exactly once

#### Scenario: No messages
- **WHEN** no messages have been published
- **THEN** the command succeeds and lists nothing

### Requirement: Consumers and cursors
`msgq consumers [--topic T]` SHALL list consumer names with their topic and offset. `msgq consumers rm <name> [--topic T]` SHALL remove matching cursors. `msgq cursor get|set|reset` SHALL read, set, or clear a consumer cursor.

#### Scenario: Monotonic cursor
- **WHEN** a cursor is set to a lower offset than its current value
- **THEN** the stored offset does not decrease

#### Scenario: Consumer removal
- **WHEN** a consumer is removed
- **THEN** it no longer appears in the consumers listing

### Requirement: Database info and protocol metadata
`msgq db info` SHALL report the resolved path, existence, size, per-stream message counts, consumer counts, and the bus protocol version. The bus SHALL record `protocol_version` and `created_by` in a table it owns and SHALL NOT modify the engine's internal tables on the write path.

#### Scenario: New database records protocol version
- **WHEN** the database is created
- **THEN** protocol version 1 is recorded

#### Scenario: Info reports provenance
- **WHEN** `msgq db info` runs
- **THEN** it reports the protocol version and the resolved file path

### Requirement: Output modes and exit codes
Commands SHALL print human-readable output by default and raw stored records as JSON lines when `--json` is provided. The exit status SHALL be 0 on success, 1 on a runtime or database error, and 2 on a usage error.

#### Scenario: JSON output
- **WHEN** `--json` is provided to read or follow
- **THEN** each record is emitted as one JSON object per line

#### Scenario: Usage error
- **WHEN** an unknown flag is provided
- **THEN** the CLI exits with status 2

### Requirement: Concurrency safety
Only one process SHALL follow a given consumer and address pair at a time. The CLI SHALL take an advisory lock for the lifetime of a follow and SHALL fail loudly when another process holds it. The CLI SHALL rely on WAL mode and a bounded busy timeout so concurrent publishers and readers remain consistent.

#### Scenario: Duplicate follow rejected
- **WHEN** a second follow starts for the same consumer and address
- **THEN** it exits with an error instead of sharing the cursor

#### Scenario: Concurrent publish and follow
- **WHEN** a publisher and a follower run at the same time
- **THEN** both succeed

### Requirement: File permissions
When the CLI creates the database, it SHALL restrict its permissions to the owner. It SHALL NOT tighten permissions on a database it did not create.

#### Scenario: Owner-only on creation
- **WHEN** the CLI creates a new database
- **THEN** the file is readable and writable only by its owner
