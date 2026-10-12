# msgq-extension Specification

## Purpose

A pi extension that consumes the msgq message bus and delivers its records into a running
session under a subscriber-controlled policy, so scripts and operators can reach a pi session
through the CLI without owning its process.

## Requirements

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
### Requirement: Delivery policy
The extension SHALL apply a subscriber policy that caps publisher intent. Under the `display` policy, every record SHALL be delivered as a display-only message and SHALL NOT start a turn. Under the `honor` policy, the record's `type` SHALL determine delivery, but only for senders the session trusts: when a trusted sender set is configured, a record from any other sender SHALL be delivered display-only. The default policy SHALL be `display`. An unrecognized policy value SHALL disable the extension with a clear message.

#### Scenario: Display is the default
- **WHEN** no policy is configured and a record of type `prompt` arrives
- **THEN** it is delivered as a display-only message and no turn starts

#### Scenario: Honor enables turns
- **WHEN** the policy is `honor` and a record of type `prompt` arrives while the agent is idle
- **THEN** a turn starts

#### Scenario: Untrusted sender
- **WHEN** the policy is `honor`, a trusted sender set is configured, and a record arrives from a sender outside it
- **THEN** it is delivered display-only and no turn starts

#### Scenario: Trusted sender
- **WHEN** the policy is `honor` and a record arrives from a sender inside the trusted set
- **THEN** its type determines delivery

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
### Requirement: Presence registration
A live session SHALL register a time-limited lease in the bus and renew it while it runs. The
lease SHALL be renewed often enough that a single missed renewal does not make the session appear
offline, and SHALL be released when the session shuts down. Presence registration SHALL be
enabled by default and MAY be disabled by configuration.

#### Scenario: Registered on start
- **WHEN** a session starts and presence is enabled
- **THEN** a lease identifying that session exists in the bus

#### Scenario: Renewal keeps it live
- **WHEN** the session keeps running
- **THEN** its lease remains unexpired without further action

#### Scenario: Released on clean shutdown
- **WHEN** the session shuts down cleanly
- **THEN** its lease is released

#### Scenario: Crash ages out
- **WHEN** the session stops without releasing its lease
- **THEN** the lease expires on its own

#### Scenario: Presence disabled
- **WHEN** presence is disabled by configuration
- **THEN** no lease is created and message consumption still starts
### Requirement: Presence identity
The lease SHALL identify the session by its session identifier and SHALL carry the session alias
when one is set, using the same identity the extension already uses for its consumer cursors. The
lease SHALL be distinguishable from every other lease in the bus.

#### Scenario: Unnamed session
- **WHEN** a session has no alias
- **THEN** its lease is identified by the session identifier

#### Scenario: Aliased session
- **WHEN** a session has an alias
- **THEN** the lease carries that alias and the session is discoverable by it

#### Scenario: Shared alias
- **WHEN** two sessions use the same alias
- **THEN** they hold distinct leases and neither displaces the other
### Requirement: Presence state
The lease SHALL carry the session state, which SHALL be one of `idle`, `busy`, or `blocked`. The
state SHALL become busy while the agent is running, return to idle once the agent settles, and
become blocked while the session waits on a user-facing prompt. A state change SHALL be recorded
promptly rather than waiting for the next renewal. A mid-session rename SHALL be reflected in the
session's alias and address.

#### Scenario: Idle at rest
- **WHEN** the session is idle
- **THEN** its state is `idle`

#### Scenario: Busy during a run
- **WHEN** an agent run starts
- **THEN** the state becomes `busy` promptly

#### Scenario: Idle after a run
- **WHEN** the agent settles
- **THEN** the state returns to `idle`

#### Scenario: Blocked on a prompt
- **WHEN** the session waits on a user-facing prompt
- **THEN** the state becomes `blocked` and returns when the prompt closes

#### Scenario: Renamed mid-session
- **WHEN** the session is renamed while it is running
- **THEN** its presence reflects the new alias without a restart
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
### Requirement: Presence resilience
A failure to acquire, renew, or release a lease SHALL NOT stop message consumption and SHALL NOT
crash the session. The extension SHALL attempt to re-acquire a lease it no longer holds, and SHALL
report the condition where the session has a UI.

#### Scenario: Renewal lost
- **WHEN** a renewal reports that the lease is no longer held
- **THEN** the extension attempts to re-acquire and continues consuming

#### Scenario: Acquire fails
- **WHEN** the lease cannot be acquired
- **THEN** consumption still starts and the condition is reported

#### Scenario: Lease squatted
- **WHEN** another process holds the session's lease
- **THEN** consumption continues and the condition is reported
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
### Requirement: Peer tool
The extension SHALL expose a tool that lets the model work with other agents on the bus. It SHALL
be able to list the reachable peers, send a message, ask a question and receive the answer, and
reply to the message it most recently received. Sending SHALL go through the bus and SHALL NOT
require the peer to have been started by this session.

#### Scenario: Listing peers
- **WHEN** the model lists peers
- **THEN** it receives the addresses currently reachable on the bus

#### Scenario: Asking and receiving
- **WHEN** the model asks a peer a question
- **THEN** the tool returns the peer's reply

#### Scenario: Replying to the latest message
- **WHEN** the model replies without naming a recipient
- **THEN** the reply is addressed to the sender of the message it most recently received

#### Scenario: Any peer, not only companions
- **WHEN** the model sends to an address it did not start itself
- **THEN** the message is sent like any other
### Requirement: Companion tool
The extension SHALL expose a tool that starts and closes companions by delegating to the companion
command. Starting SHALL report the granted name and the address the companion can be reached at.

#### Scenario: Starting a companion
- **WHEN** the model starts a companion
- **THEN** the tool reports its granted name and its bus address

#### Scenario: Closing a companion
- **WHEN** the model closes a companion
- **THEN** its tab is closed

#### Scenario: Failure is reported
- **WHEN** the companion command fails
- **THEN** the tool reports the failure instead of returning a name
### Requirement: Reporting address
The extension SHALL accept a reporting address from configuration and from the environment, and
SHALL use it as the default recipient when a tool call does not name one.

#### Scenario: Default recipient
- **WHEN** a reporting address is configured and the model sends without naming a recipient
- **THEN** the message goes to the reporting address

#### Scenario: Environment is honoured
- **WHEN** the environment carries a parent address and no configuration overrides it
- **THEN** it is used as the reporting address
