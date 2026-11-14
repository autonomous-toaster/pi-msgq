# msgq-cli Specification

## Purpose

`msgq` is a durable, topic-addressed message bus over a single local SQLite file, letting scripts, humans, and (later) pi sessions publish and consume messages without a daemon, broker, or network service.

## Requirements

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

### Requirement: File permissions
When the CLI creates the database, it SHALL restrict its permissions to the owner. It SHALL NOT tighten permissions on a database it did not create.

#### Scenario: Owner-only on creation
- **WHEN** the CLI creates a new database
- **THEN** the file is readable and writable only by its owner

### Requirement: Sessions listing
`msgq sessions` SHALL list the live participants recorded as unexpired presence records in the bus.
Each entry SHALL show the participant's address, kind, state, and remaining lifetime. Records older
than the participant's declared lifetime SHALL be excluded, at most one entry SHALL be shown per
participant, and `--json` SHALL emit one JSON object per participant. The listing SHALL be derived from
records alone, so that any participant able to publish is listed, whether or not it is a pi session.
When a participant's newest presence record carries a locator card, `--json` SHALL include that card,
and the text output SHALL show its locator fields only when `--wide` is requested. The default text
output SHALL remain unchanged.

#### Scenario: Lists a live session
- **WHEN** a participant has published a presence record that has not yet expired
- **THEN** `msgq sessions` shows its address and its state

#### Scenario: Any participant can appear
- **WHEN** a process publishes a presence record using nothing but the command-line client
- **THEN** it is listed alongside pi sessions

#### Scenario: Excludes expired leases
- **WHEN** a presence record is older than the lifetime it declares
- **THEN** that participant is not listed

#### Scenario: Retraction is immediate
- **WHEN** a participant retracts its presence before shutting down
- **THEN** it is not listed, without waiting for the lifetime to elapse

#### Scenario: One entry per participant
- **WHEN** more than one presence record exists for the same participant
- **THEN** the listing shows a single entry for it, reflecting the most active state and the newest record

#### Scenario: Kind is reported
- **WHEN** participants of more than one kind are present
- **THEN** each entry reports its kind

#### Scenario: JSON output
- **WHEN** `--json` is given
- **THEN** each participant is emitted as one JSON object per line, with the same fields as before and a `card` field when one is present

#### Scenario: Wide output
- **WHEN** `--wide` is given
- **THEN** the text output appends exactly one locator column holding the card's project, branch, and path, in that order

#### Scenario: The locator column omits repetition
- **WHEN** the card's `worktree` repeats the repository name already inside `project`, or its `path` is `.`
- **THEN** neither is repeated in the locator column, which shows only what tells peers apart

#### Scenario: No card
- **WHEN** a live participant has no card, or an empty one
- **THEN** the locator column is `-` and the row keeps its five other columns

#### Scenario: Default text output is unchanged
- **WHEN** `--wide` is not given
- **THEN** the text output has the same columns as before, whether or not the participant has a card

#### Scenario: Nobody online
- **WHEN** no presence record is live
- **THEN** the command succeeds and lists nothing

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

With `--no-wait` the command SHALL publish the same record and return as soon as it is published,
printing its offset, without waiting for any reply. The published record SHALL be identical to the
one a blocking ask publishes, so a client cannot tell the two apart on the wire.

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

#### Scenario: Asking without waiting
- **WHEN** `--no-wait` is provided
- **THEN** the command prints the offset of the published question and returns before any reply exists

#### Scenario: One question, one shape
- **WHEN** the same question is asked with and without `--no-wait`
- **THEN** the two records differ only in the offset they are stored at

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

### Requirement: Cursor collection

Cursors belonging to participants that are no longer live SHALL be removable, so that a bus used for
months does not accumulate one cursor per address per dead session.

#### Scenario: Dead participant's cursors go
- **WHEN** cursors are collected and the participant has no live presence record
- **THEN** its cursors are removed

#### Scenario: Live participants are untouched
- **WHEN** cursors are collected and the participant is live
- **THEN** its cursors are kept, including the one that would otherwise look idle

#### Scenario: Collecting is reported
- **WHEN** cursors are collected
- **THEN** the number removed is reported

### Requirement: Peer locator derivation

`msgq card` SHALL print one flat JSON object describing the current working directory as a peer
locator. It SHALL derive the locator from git when the directory is inside a work tree, and SHALL
succeed with a smaller object when git is unavailable or the directory is not a repository. A bad
`--cwd` value SHALL be a usage error.

The command SHALL accept `--cwd PATH` and `--paths relative|full|none`. `relative` is the default and
puts the path relative to the repository root in `path`; `full` puts the absolute current directory
there; `none` omits `path`. Outside git, `branch` and `worktree` SHALL be omitted; under `full`,
`path` SHALL still be the absolute current directory.

`project` SHALL be the normalized `origin` remote as `host/owner/repo` with any trailing `.git`
removed when an origin exists, otherwise the repository root directory name, or the current directory
name outside git.

`msgq card --address ADDR` SHALL additionally report, as one JSON object, the canonical `address`,
the `card`, and the `locator` — the single column `msgq sessions --wide` shows. It SHALL NOT merge the
address into the card, because the published `event.card` has four fields and no address. An address
using an unknown scheme SHALL be a usage error. Without `--address`, the output SHALL remain the
plain card object.

#### Scenario: A git repository

- **WHEN** `msgq card` runs in a git repository with an origin
- **THEN** it reports the normalized origin as `project`, the current branch, the worktree name, and a relative path

#### Scenario: A subdirectory

- **WHEN** `msgq card` runs in a subdirectory of a git repository
- **THEN** `path` is relative to the repository root and `worktree` names the root directory

#### Scenario: No git repository

- **WHEN** `msgq card` runs outside git
- **THEN** it reports the current directory name as `project` and omits `branch` and `worktree`

#### Scenario: Remote normalization

- **WHEN** the origin remote is an HTTPS or SSH URL
- **THEN** `project` is the same `host/owner/repo` value for both forms

#### Scenario: Path modes

- **WHEN** `--paths` is `relative`, `full`, or `none`
- **THEN** `path` is respectively relative to the root, the absolute current directory, or absent

#### Scenario: Address beside the locator
- **WHEN** `msgq card --address session/planner` runs
- **THEN** it reports the address, the card, and the locator line, and the card is still published unchanged

#### Scenario: Detached HEAD

- **WHEN** the repository is on a detached HEAD
- **THEN** the locator omits `branch` and still reports the project and worktree

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

### Requirement: Cursors and consumers stay inside the caller's namespace

T3.4 ALWAYS, the consumers listing SHALL name only the consumers owned by the caller.
T3.4 ALWAYS, a cursor command SHALL act on a cursor row owned by the caller.
T3.4 ALWAYS, the consumers listing SHALL show the owning principal.

#### Scenario: Another tenant's consumer is absent
- **WHEN** a consumer of another principal holds a cursor on an address
- **THEN** the listing SHALL NOT name that consumer

### Requirement: Prune reports the removal the caller may perform

T3.5 ALWAYS, prune SHALL remove records only from an address inside the scopes of the caller.
T3.5 ALWAYS, prune SHALL report the total removed.
T3.5 ALWAYS, a refused prune SHALL leave the bus unchanged.

#### Scenario: A refused prune changes nothing
- **WHEN** prune names an address outside the scopes of the caller
- **THEN** the CLI SHALL exit non-zero and SHALL remove no record

### Requirement: The identity report names the caller

T4.2 ALWAYS, db info SHALL report the principal of the caller.
T4.2 ALWAYS, db info SHALL report the scopes of the caller.

#### Scenario: The report names the principal
- **WHEN** a client runs db info against a shared engine
- **THEN** the report SHALL name the principal and the scopes of the client

### Requirement: Publish seals the body

T3.1 IF a key is configured, THEN the CLI SHALL seal the body on publish.
T3.1 IF no key is configured, THEN the CLI SHALL store a plaintext body.
T3.1 IF no key is configured, THEN the CLI SHALL report that the body is plaintext.

#### Scenario: A configured key seals
- **WHEN** an operator publishes with a key configured
- **THEN** the stored body SHALL be sealed

### Requirement: Read opens the body

T3.2 IF the body opens, THEN the CLI SHALL print the content.
T3.2 IF the body fails to open, THEN the CLI SHALL exit non-zero.
T3.2 IF the body fails to open, THEN the CLI SHALL print no body.
T3.2 IF the body fails to open, THEN the CLI SHALL name the key identifier.

#### Scenario: An unopenable body is an error
- **WHEN** an operator reads a sealed record without the key
- **THEN** the CLI SHALL exit non-zero and SHALL print no body

### Requirement: A listing has a names form

T2.1 ALWAYS, `msgq topics --names` SHALL print one address per line.
T2.1 ALWAYS, `msgq sessions --names` SHALL print one participant address per line.
T2.1 ALWAYS, `msgq consumers --names` SHALL print one consumer name per line.
T2.1 ALWAYS, a names form SHALL print no count, no header, no offset and no JSON.

#### Scenario: Names are addressable
- **WHEN** a caller runs `msgq topics --names` on a bus that holds two addresses
- **THEN** the output SHALL hold exactly the two addresses, one per line

#### Scenario: Names carry no bookkeeping
- **WHEN** a caller runs a names form
- **THEN** the output SHALL hold no column separator and no header line


#### Scenario: Names respect the caller's namespace
- **WHEN** a caller runs `msgq consumers --names` on a bus that holds a cursor of another principal
- **THEN** the output SHALL name no consumer of that principal

### Requirement: Two output forms are refused together

T2.2 IF a caller asks for a names form and JSON together, THEN the CLI SHALL exit with a usage error.

#### Scenario: Two output forms are refused together
- **WHEN** a caller requests a names form and JSON
- **THEN** the CLI SHALL exit with a usage error

### Requirement: A closed value is accepted from the members of a closed set

T1.1 ALWAYS, the CLI SHALL accept a value of a closed set from the members of that closed set.
T1.2 ALWAYS, the CLI SHALL validate an acknowledgement phase set where the argument is parsed.
T1.3 ALWAYS, the help text SHALL name the members of a closed set.

#### Scenario: The help names the members
- **WHEN** a caller reads the help of a command with a closed-set argument
- **THEN** the help SHALL name every member of that closed set

### Requirement: A value outside a closed set is refused

T1.1 IF a supplied value is not a member of the closed set of the argument, THEN the CLI SHALL exit with a usage error.

#### Scenario: An unknown record type
- **WHEN** a caller publishes with `--type` naming a value outside the members
- **THEN** the CLI SHALL exit with a usage error and SHALL store no record

#### Scenario: An unknown acknowledgement phase
- **WHEN** a caller publishes with `--ack` naming a phase outside the protocol phases
- **THEN** the CLI SHALL exit with a usage error and SHALL store no record

### Requirement: The completion verb describes one shell

T3.1 ALWAYS, `msgq completions <shell>` SHALL accept `zsh` alone.

#### Scenario: The supported shell is named in the help
- **WHEN** a caller reads the help of `msgq completions`
- **THEN** the help SHALL name `zsh`

### Requirement: An unknown shell is refused

T4.2 IF the caller names another shell, THEN the CLI SHALL exit with a usage error.
T4.2 IF the caller names no shell, THEN the CLI SHALL exit with a usage error.

#### Scenario: Another shell is refused
- **WHEN** a caller asks for a bash completion
- **THEN** the CLI SHALL exit with a usage error and SHALL print no script

### Requirement: A listing has a card form

T2.3 ALWAYS, `msgq topics --card` SHALL print two tab-separated columns: the address, then a compact description.
T2.3 ALWAYS, `msgq sessions --card` SHALL print the address, then the compact description of the card.
T2.3 ALWAYS, the compact description SHALL name the project and the branch, separated by a slash.
T2.3 ALWAYS, the compact description SHALL name the last component of the path.
T2.3 ALWAYS, the compact description SHALL contain no separator inside either column.

#### Scenario: An address with a card
- **WHEN** a participant advertises an address with a card naming project, branch and path
- **THEN** `--card` SHALL print the address, a tab, and the project and branch with the last path component

#### Scenario: Two columns are cut cleanly
- **WHEN** a caller cuts the first column of a card form
- **THEN** the output SHALL hold one address per line, whatever the second column holds

### Requirement: A card description is bounded

T2.5 ALWAYS, the compact description SHALL fit a bounded width whatever the length of a name.
T2.5 ALWAYS, a shortened name SHALL keep both ends and lose the middle.
T2.5 ALWAYS, the address SHALL stand first in the row. T4.1, T4.2

#### Scenario: A long repository name
- **WHEN** a card names a project and a branch longer than the budget
- **THEN** the description SHALL be shortened in the middle and SHALL stay within the budget

#### Scenario: A narrow terminal keeps the address
- **WHEN** a picker shows the card form in a terminal narrower than the row
- **THEN** the truncated part SHALL be the description, not the address

### Requirement: A card form falls back rather than varying

T2.4 IF an address has no live participant, THEN the card form SHALL print `-` in the second column.
T2.4 IF a card names neither project, branch nor path, THEN the card form SHALL print `-` in the second column. T2.1, T2.2, T2.3
T2.4 IF a caller asks for two output forms together, THEN the CLI SHALL exit with a usage error.

#### Scenario: A topic no participant advertises
- **WHEN** a caller runs the card form on a bus where no participant advertises one of the addresses
- **THEN** that address SHALL be printed with `-` as its description

#### Scenario: Three forms are refused in pairs
- **WHEN** a caller asks for the card form together with `--names` or `--json`
- **THEN** the CLI SHALL exit with a usage error and SHALL print no listing
