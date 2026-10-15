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
`msgq publish <address> [message]` SHALL append one message to the stream named by the canonical address and print the assigned offset to stdout. It SHALL be able to report the record's id instead of that offset, to request acknowledgements on the record it publishes, and to reference the message it is replying to, by offset or by id.

#### Scenario: Successful publish
- **WHEN** a message is published
- **THEN** the assigned offset is printed and the message is returned by a subsequent read or follow

#### Scenario: Default sender
- **WHEN** `--from` is omitted
- **THEN** the record's `from` is `cli/<user>@<host>`

#### Scenario: Body from stdin
- **WHEN** the message argument is `-`
- **THEN** the body is read from stdin

#### Scenario: Requesting an acknowledgement
- **WHEN** `--ack` is given
- **THEN** the published record requests acknowledgement of every phase, or of the phases named when a list is given

#### Scenario: Replying to a message
- **WHEN** `--reply-to` is given
- **THEN** the published record carries that offset as its reply reference

#### Scenario: Reporting the id
- **WHEN** the id is asked for instead of the bare offset
- **THEN** the printed value is the canonical address and the assigned offset joined by `#`

#### Scenario: Referencing by id
- **WHEN** `--reply-to` names an id whose address is the record's own sender
- **THEN** the published record carries that id's offset as its reply reference

#### Scenario: Referencing another sender's id
- **WHEN** `--reply-to` names an id whose address is not the record's own sender
- **THEN** the command exits with a usage error and writes nothing
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
Commands SHALL print human-readable output by default and raw stored records as JSON lines when
`--json` is provided. The exit status SHALL be 0 on success, 1 on a runtime or database error,
2 on a usage error, and 3 when an await finishes without a terminal acknowledgement.

#### Scenario: JSON output
- **WHEN** `--json` is provided to read or follow
- **THEN** each record is emitted as one JSON object per line

#### Scenario: Usage error
- **WHEN** an unknown flag is provided
- **THEN** the CLI exits with status 2

#### Scenario: Await timeout
- **WHEN** an await finishes without a terminal acknowledgement
- **THEN** the CLI exits with status 3
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
### Requirement: Sessions listing
`msgq sessions` SHALL list the live participants recorded as unexpired leases in the bus. Each
entry SHALL show the participant's address, kind, state, and remaining lifetime. Expired leases
SHALL be excluded, at most one entry SHALL be shown per participant, and `--json` SHALL emit one
JSON object per participant.

#### Scenario: Lists a live session
- **WHEN** a session holds an unexpired lease
- **THEN** `msgq sessions` shows its address and its state

#### Scenario: Excludes expired leases
- **WHEN** a lease has expired
- **THEN** that participant is not listed

#### Scenario: One entry per participant
- **WHEN** more than one lease row exists for the same participant
- **THEN** the listing shows a single entry for it

#### Scenario: Kind is reported
- **WHEN** participants of more than one kind are present
- **THEN** each entry reports its kind

#### Scenario: JSON output
- **WHEN** `--json` is given
- **THEN** each participant is emitted as one JSON object per line

#### Scenario: Nobody online
- **WHEN** no lease is live
- **THEN** the command succeeds and lists nothing
### Requirement: Awaiting acknowledgements
`msgq send --await <address> [message]` SHALL publish the message and then wait for
acknowledgements of it. `msgq await <offset>` SHALL wait for acknowledgements of an offset that was
already published. Waiting SHALL begin from a position recorded before publishing, so an
acknowledgement that arrives early is still reported. Each acknowledgement SHALL be reported as it
arrives, and the command SHALL finish when a terminal acknowledgement is seen. When no address is
given, the wait SHALL address itself to the sender recorded on the message at that offset, so the
caller does not have to know where the acknowledgements will land. `msgq await --json`
SHALL emit one JSON object per acknowledgement; `msgq send --await` prints human-readable phases,
because `--json` on `publish` already selects JSON record input.

#### Scenario: Phases are reported as they arrive
- **WHEN** an awaited message is started and then completed by its session
- **THEN** the command reports the `started` and `completed` acknowledgements

#### Scenario: An early acknowledgement is not missed
- **WHEN** an acknowledgement arrives between publishing and the wait beginning
- **THEN** it is still reported

#### Scenario: Reply address is derived
- **WHEN** no address is given, and the message at that offset was sent by a known address
- **THEN** the wait follows that address

#### Scenario: Terminal phase ends the wait
- **WHEN** a terminal acknowledgement arrives
- **THEN** the command finishes successfully

#### Scenario: Display delivery ends the wait
- **WHEN** the awaited message was delivered display-only
- **THEN** the wait finishes at its `received` acknowledgement

#### Scenario: Timeout
- **WHEN** no terminal acknowledgement arrives within the wait window
- **THEN** the command stops waiting and reports that it timed out

#### Scenario: Unbounded wait
- **WHEN** the wait window is set to zero
- **THEN** the command waits without a limit

#### Scenario: JSON output
- **WHEN** `msgq await --json` is given
- **THEN** each acknowledgement is emitted as one JSON object per line
### Requirement: Acknowledgement rendering
Human-readable output SHALL render an acknowledgement as a lifecycle line rather than a raw JSON
blob, naming it as an acknowledgement and showing the offset it refers to, its phase, and any
delivery or outcome detail. JSON output SHALL remain the raw stored record.

#### Scenario: Lifecycle line
- **WHEN** an acknowledgement is printed in human-readable output
- **THEN** the line names it as an acknowledgement and shows the referenced offset, the phase, and any detail

#### Scenario: JSON unchanged
- **WHEN** `--json` is given
- **THEN** the acknowledgement is printed as its raw stored record
### Requirement: Awaiting a reply
`msgq ask <address> [message]` SHALL publish a message that asks for an answer and then wait for
one. It SHALL finish when the recipient replies, when a terminal acknowledgement for that message
arrives, or when its wait window closes. A reply SHALL be printed as the command's output; the
other two outcomes SHALL be reported as such. None of the three SHALL be an error. The wait window
SHALL default to a finite value and MAY be disabled.

#### Scenario: Reply arrives
- **WHEN** the recipient replies to the message
- **THEN** the reply is printed and the command succeeds

#### Scenario: Finished without replying
- **WHEN** a terminal acknowledgement for the message arrives and no reply does
- **THEN** the command reports that the recipient finished without replying and succeeds

#### Scenario: Still running
- **WHEN** the wait window closes with neither a reply nor a terminal acknowledgement
- **THEN** the command reports that the recipient is still running and succeeds

#### Scenario: Wait window disabled
- **WHEN** the wait window is disabled
- **THEN** the command waits until the recipient replies or finishes
### Requirement: Record identity
Every record SHALL have an id of the form `<address>#<offset>`: the canonical address whose stream
holds it, and the offset it was assigned. The id SHALL be derived from what the
record already carries and SHALL NOT be stored in the record, so it applies to every record ever
published. A record's reply reference SHALL be an offset in the stream named by that record's own
`from`, so the record a reference names is always `<from>#<reply_to>`.

#### Scenario: Derived, not stored
- **WHEN** a record is published
- **THEN** its id is the canonical address it was published to and the offset assigned there, and the stored record carries no id field

#### Scenario: A reference identifies its record
- **WHEN** a published record carries `reply_to`
- **THEN** the record it refers to is the one at that offset in the stream named by that record's `from`

#### Scenario: A reference is exact rather than inferred
- **WHEN** a reply is published to the address of the session that sent the record it answers
- **THEN** its `reply_to` is the offset of that record in the answering session's stream, which is the stream named by the reply's `from`

#### Scenario: Earlier records have ids too
- **WHEN** a record published before this change is read
- **THEN** an id can be formed for it from its address and its offset
