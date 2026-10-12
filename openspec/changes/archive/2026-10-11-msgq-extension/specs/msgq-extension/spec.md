## Purpose

A pi extension that consumes the msgq message bus and delivers its records into a running
session under a subscriber-controlled policy, so scripts and operators can reach a pi session
through the CLI without owning its process.

## ADDED Requirements

### Requirement: Loading and lifecycle
The extension SHALL be loadable ad hoc from a directory with `pi -e <path>` without being
installed or published. It SHALL begin its work on the session start event and SHALL NOT start
long-lived resources when its factory runs. It SHALL release all resources on session shutdown,
and cleanup SHALL be idempotent across reload, session replacement, and process exit.

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

### Requirement: Bus access
The extension SHALL consume the message bus at the same location the CLI uses: the `MSGQ_DB`
environment variable when set, otherwise the default `~/.pi/agent/msgq/msgq.db`. It SHALL use
the engine's own API rather than hardcoded table access, SHALL create the bus through the engine
when the file does not exist, and SHALL close the engine handle on shutdown.

#### Scenario: Shared location
- **WHEN** the CLI has published to the default path and the extension starts without `MSGQ_DB`
- **THEN** the extension reads from that same bus

#### Scenario: Environment override
- **WHEN** `MSGQ_DB` is set
- **THEN** the extension uses that path for the whole session

#### Scenario: Bus created on first use
- **WHEN** the extension starts and the bus file does not exist
- **THEN** the engine creates it and the extension proceeds

### Requirement: Default subscription
The extension SHALL subscribe to the session's own address `session/<self>`, where `<self>` is
the session alias when one is set, otherwise the session identifier.

#### Scenario: Aliased session
- **WHEN** the session has the alias `api-worker`
- **THEN** the extension subscribes to `session/api-worker`

#### Scenario: Unnamed session
- **WHEN** the session has no alias
- **THEN** the extension subscribes to an address derived from the session identifier

### Requirement: Additional subscriptions
The extension SHALL accept additional addresses from configuration, as a comma-separated list
supplied by both a command-line flag and an environment variable. Each additional address SHALL
be subscribed independently, and an invalid address SHALL disable the extension with a clear
message rather than being ignored.

#### Scenario: Extra group topics
- **WHEN** configuration lists `group/backend` and `group/ci`
- **THEN** the extension subscribes to both in addition to its own address

#### Scenario: Invalid address
- **WHEN** configuration contains an address with an unknown scheme
- **THEN** the extension reports the error and does not start consuming

### Requirement: Consumer identity and isolation
The extension SHALL use a consumer identity that is unique per session, formed from the session
alias when present together with the session identifier. Two sessions SHALL NOT share a cursor
for the same address.

#### Scenario: Duplicate aliases
- **WHEN** two sessions both use the alias `worker`
- **THEN** each has its own consumer identity and advancing one does not advance the other

### Requirement: Cursor durability
The extension SHALL persist the consumer offset before a record is reported as delivered, so an
interruption replays at most one record. A consumer with no saved offset SHALL start at the
current end of its stream.

#### Scenario: Save before report
- **WHEN** a record is delivered
- **THEN** its offset is already persisted before delivery is observable

#### Scenario: No history replay on first attach
- **WHEN** a session subscribes to an address that already has records
- **THEN** previously published records are not replayed

### Requirement: Delivery policy
The extension SHALL apply a subscriber policy that caps publisher intent. Under the `display`
policy, every record SHALL be delivered as a display-only message and SHALL NOT start a turn.
Under the `honor` policy, the record's `type` SHALL determine delivery. The default policy SHALL
be `display`. An unrecognized policy value SHALL disable the extension with a clear message.

#### Scenario: Display is the default
- **WHEN** no policy is configured and a record of type `prompt` arrives
- **THEN** it is delivered as a display-only message and no turn starts

#### Scenario: Honor enables turns
- **WHEN** the policy is `honor` and a record of type `prompt` arrives while the agent is idle
- **THEN** a turn starts

#### Scenario: Unknown policy rejected
- **WHEN** configuration sets an unrecognized policy
- **THEN** the extension reports the error and does not start consuming

### Requirement: Record decoding
The extension SHALL decode the record envelope and SHALL tolerate types and fields it does not
recognize, delivering unknown types as display-only messages. A payload that cannot be decoded
SHALL be skipped without stopping consumption, and the skip SHALL be reported.

#### Scenario: Unknown type is tolerated
- **WHEN** a record has a `type` the extension does not recognize
- **THEN** it is displayed without error and consumption continues

#### Scenario: Malformed payload
- **WHEN** a stream entry cannot be decoded as a record
- **THEN** it is skipped, reported, and consumption continues

### Requirement: Display-only delivery
The extension SHALL deliver display-only messages as custom messages that are visible in the
transcript and SHALL NOT start a turn. Display-only delivery SHALL be safe to perform while the
agent is running.

#### Scenario: Note while idle
- **WHEN** a `note` record arrives while the agent is idle
- **THEN** it appears in the transcript and no turn starts

#### Scenario: Note while streaming
- **WHEN** a `note` record arrives while the agent is running
- **THEN** it appears in the transcript once the current turn ends and no new turn is queued

### Requirement: Provenance in delivered messages
Every delivered message SHALL identify the sender address and the address it was received on, so
the model can distinguish external messages from user input.

#### Scenario: Sender is visible
- **WHEN** a record published from `cli/alice@mac` to `session/api-worker` is delivered
- **THEN** the delivered message names both `cli/alice@mac` and `session/api-worker`

### Requirement: Transcript rendering
The extension SHALL register a renderer for its message type so delivered messages read as
structured transcript entries rather than raw text. The renderer SHALL show the bus label, the
record type, the sender-to-address route, and the body. A coalesced batch SHALL show the number
of records and number each one. Rendering SHALL be presentation-only: the plain content used
outside the interactive transcript SHALL be unchanged, and a message without structured details
SHALL still render.

#### Scenario: Single message
- **WHEN** one record is delivered in the interactive transcript
- **THEN** the entry shows the label, the record type, the sender-to-address route, and the body

#### Scenario: Coalesced batch
- **WHEN** several records are delivered together
- **THEN** the entry shows the record count and numbers each record with its own route and body

#### Scenario: Presentation-only
- **WHEN** a session runs without an interactive transcript
- **THEN** the delivered message content is the same plain text as before

#### Scenario: Missing details
- **WHEN** a delivered message carries no structured details
- **THEN** the renderer falls back to the message content instead of failing

### Requirement: Turn-bearing delivery under honor
Under the `honor` policy, a `prompt` record SHALL start a turn when the agent is idle. While the
agent is running, `steer` and `follow_up` records SHALL be delivered using the corresponding
pi delivery modes, and a `prompt` record SHALL be treated as follow-up work.

#### Scenario: Prompt starts a turn
- **WHEN** the policy is `honor`, the agent is idle, and a `prompt` record arrives
- **THEN** a turn starts

#### Scenario: Steer mid-run
- **WHEN** the policy is `honor`, the agent is running, and a `steer` record arrives
- **THEN** it is delivered at the next safe model boundary without aborting the run

#### Scenario: Follow-up after the run
- **WHEN** the policy is `honor`, the agent is running, and a `follow_up` record arrives
- **THEN** it is delivered after the current run finishes

### Requirement: Coalescing of turn-bearing records
Turn-bearing records that arrive while the agent is running SHALL be buffered and delivered as a
single message, and SHALL cause at most one additional model request.

#### Scenario: Many records, one continuation
- **WHEN** several turn-bearing records arrive during one run under the `honor` policy
- **THEN** they are delivered together as one message and exactly one continuation follows

### Requirement: Self-trigger guard
A record whose sender address equals the session's own address SHALL NOT start a turn.

#### Scenario: Self-addressed record
- **WHEN** a record arrives whose `from` equals the session's own address under the `honor` policy
- **THEN** it is delivered as display-only and no turn starts

### Requirement: Graceful degradation
When the bus is unreadable or the record version is unsupported, the extension SHALL NOT crash
the session. It SHALL stop consuming and report the condition where the session has a UI.

#### Scenario: Unreadable bus
- **WHEN** the bus cannot be opened
- **THEN** the session continues and the extension reports that it is disabled

#### Scenario: Unsupported record version
- **WHEN** a record carries a protocol version the extension does not support
- **THEN** the record is skipped, reported, and consumption continues
