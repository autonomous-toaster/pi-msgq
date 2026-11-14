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
record type, the sender-to-address route, and the body. It SHALL also show how a human answers that
message, naming the sender the answer would go to, and that affordance SHALL be part of the
rendering only. A coalesced batch SHALL show the number of records and number each one. Rendering
SHALL be presentation-only: the plain content used outside the interactive transcript SHALL be
unchanged, and a message without structured details SHALL still render.

#### Scenario: Single message
- **WHEN** one record is delivered in the interactive transcript
- **THEN** the entry shows the label, the record type, the sender-to-address route, and the body

#### Scenario: Reply affordance
- **WHEN** a record from another address is delivered in the interactive transcript
- **THEN** the entry names the sender an answer would go to, so a human can reply without looking it up

#### Scenario: Coalesced batch
- **WHEN** several records are delivered together
- **THEN** the entry shows the record count and numbers each record with its own route and body

#### Scenario: Presentation-only
- **WHEN** a session runs without an interactive transcript
- **THEN** the delivered message content is the same plain text as before, with no reply affordance in it

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

### Requirement: Presence identity
The presence record SHALL identify the session by its session identifier and SHALL carry the session
alias when one is set, using the same identity the extension already uses for its consumer cursors.
The participant SHALL be distinguishable from every other participant in the bus.

#### Scenario: Unnamed session
- **WHEN** a session has no alias
- **THEN** its presence is identified by the session identifier

#### Scenario: Aliased session
- **WHEN** a session has an alias
- **THEN** the presence record carries that alias and the session is discoverable by it

#### Scenario: Shared alias
- **WHEN** two sessions use the same alias
- **THEN** both are listed as distinct participants and neither displaces the other

### Requirement: Presence state
The presence record SHALL carry the session state, which SHALL be one of `idle`, `busy`, or
`blocked`. The state SHALL become busy while the agent is running, return to idle once the agent
settles, and become blocked while the session waits on a user-facing prompt. A state change SHALL be
published promptly rather than waiting for the next renewal. A mid-session rename SHALL be reflected
in the session's alias and address.

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
The extension SHALL publish acknowledgement records and presence records only, and SHALL NOT publish
any message record to the bus. Presence records SHALL be published to the presence address and to no
other address, so that presence never appears in a stream a session reads as conversation. Neither
kind SHALL alter the record contract, and neither SHALL advance any consumer cursor other than the
extension's own where it consumes them.

#### Scenario: No message publishing
- **WHEN** the extension runs
- **THEN** no message record is published except an acknowledgement

#### Scenario: Streams untouched
- **WHEN** presence is registered, renewed, retracted, or changes state
- **THEN** the only stream appended to is the presence address: no conversation stream offset and no consumer cursor changes as a result

#### Scenario: No arbitrary publishing
- **WHEN** the extension handles a record it was asked to acknowledge
- **THEN** the record it publishes is an acknowledgement and nothing else

### Requirement: Presence resilience
A failure to publish, renew, or retract presence SHALL NOT stop message consumption and SHALL NOT
crash the session. The extension SHALL retry on the next renewal, SHALL treat an unpublished presence
as absent rather than current, and SHALL report the condition where the session has a UI.

#### Scenario: Renewal lost
- **WHEN** a renewal cannot be published
- **THEN** the extension keeps consuming and retries at the next renewal

#### Scenario: Acquire fails
- **WHEN** the first presence record cannot be published
- **THEN** consumption still starts and the condition is reported

#### Scenario: Lease squatted
- **WHEN** another process publishes presence for this session's address
- **THEN** consumption continues, the entry is not silently doubled, and the condition is reported

#### Scenario: Presence is not claimed when it was not published
- **WHEN** a publication is not confirmed
- **THEN** the session does not describe itself as registered

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
be able to list the reachable peers, send a message, ask a question, and reply to the message it most
recently received. Sending SHALL go through the bus and SHALL NOT require the peer to have been
started by this session.

Asking SHALL NOT hold the session's turn by default: a question SHALL be published and the tool SHALL
return, and the answer SHALL arrive as an ordinary inbound message that starts a turn. The model MAY
ask to wait instead, and that wait SHALL be bounded — it SHALL NOT be possible for the model to ask
for a wait with no limit. A question addressed to this session's own address SHALL be refused.

A wait SHALL be abandoned as soon as the peer being waited on asks this session a question, and the
tool SHALL say that the peer is waiting, so that a cycle of two sessions costs no wait at all. An
answer the wait already returned SHALL be delivered once: the record the peer published SHALL be
consumed rather than injected a second time. A wait that ended without an answer SHALL suppress
nothing.

#### Scenario: Listing peers
- **WHEN** the model lists peers
- **THEN** it receives the addresses currently reachable on the bus

#### Scenario: Asking does not block
- **WHEN** the model asks a peer a question without asking to wait
- **THEN** the tool returns as soon as the question is published, and the answer arrives later as an inbound message

#### Scenario: Asking and receiving
- **WHEN** the model asks a peer a question and asks to wait
- **THEN** the tool returns the peer's reply if it arrives inside the bounded wait, and reports that the peer is still working if it does not

#### Scenario: The wait is bounded
- **WHEN** the model asks for a wait of no limit, or of longer than the extension allows
- **THEN** the wait is clamped, and the tool never waits without a bound

#### Scenario: Two sessions ask each other
- **WHEN** a session waiting on a peer receives a question from that peer
- **THEN** the wait ends immediately, the peer's question is delivered, and the tool says the peer is waiting on it

#### Scenario: One answer, once
- **WHEN** a wait returns an answer and the same record is delivered by the bus afterwards
- **THEN** it is consumed instead of being delivered again

#### Scenario: A late answer is still delivered
- **WHEN** a wait ends without an answer and the answer arrives afterwards
- **THEN** it is delivered as an ordinary inbound message

#### Scenario: Asking itself
- **WHEN** the model asks a question addressed to this session's own address
- **THEN** the tool refuses and says why

#### Scenario: Replying to the latest message
- **WHEN** the model replies without naming a recipient
- **THEN** the reply is addressed to the sender of the message it most recently received

#### Scenario: A peer this session did not start
- **WHEN** the model sends to an address it did not start itself
- **THEN** the message is sent like any other

### Requirement: Human command surface
The extension SHALL register commands a human runs from the prompt without the model: `/peers`,
`/peer`, and `/reply`. Each SHALL appear in the session's command list with a
description of what it does. A command SHALL act with the session's own identity on the same bus
the model's tools use, and the records it publishes SHALL be ordinary records: the same address
rules, the same types, and the same reply reference as any other publish. A command SHALL return
without waiting for a reply, because a reply is an inbound message and arrives by the path that
already delivers one.

#### Scenario: Commands are discoverable
- **WHEN** a human lists the session's commands
- **THEN** `/peers`, `/peer`, and `/reply` are listed with a description

#### Scenario: A command publishes an ordinary record
- **WHEN** a human sends a message with a command
- **THEN** the bus holds an ordinary record from the session's own address

#### Scenario: Human and model share one identity
- **WHEN** a human sends a message and the model sends one from the same session
- **THEN** both name the session's own address as the sender

#### Scenario: Nothing waits for a reply
- **WHEN** a human sends a message that asks a question
- **THEN** the command returns without a reply and any answer arrives later as an inbound message

### Requirement: Address resolution
Wherever a command accepts an address, `me` SHALL resolve to the session's own address, and a bare
name SHALL resolve to `session/<name>`. An address that already carries a scheme SHALL be used as
given. A command SHALL report the address it sent to, and SHALL report when that address is not
currently held by any participant, without failing.

#### Scenario: me is this session
- **WHEN** a human addresses `me`
- **THEN** it resolves to the session's own address

#### Scenario: A bare name gets its scheme
- **WHEN** a human addresses `reviewer`
- **THEN** the message is published to `session/reviewer`

#### Scenario: A full address is used as given
- **WHEN** a human addresses `cli/ci-bot`
- **THEN** the message is published to `cli/ci-bot`

#### Scenario: The address is reported
- **WHEN** a command sends a message
- **THEN** it reports the address the message was sent to

#### Scenario: Nobody is listening
- **WHEN** a command sends to an address no participant currently holds
- **THEN** it reports that the recipient was not present, and the message is still sent

### Requirement: Peers command
`/peers` SHALL list the participants currently reachable on the bus, showing each one's address and
state. Choosing a peer SHALL offer to compose a message to that peer and send it. Nothing SHALL be
sent when the composition is abandoned.

#### Scenario: The roster
- **WHEN** a human runs `/peers`
- **THEN** the addresses currently reachable on the bus are shown with their state

#### Scenario: Nobody else is reachable
- **WHEN** a human runs `/peers` and no other participant is reachable
- **THEN** the command says so rather than showing an empty result or failing

#### Scenario: Pick a peer and compose
- **WHEN** a human chooses a peer from the roster
- **THEN** they can compose a message and it is sent to that peer

#### Scenario: Composition abandoned
- **WHEN** a human abandons the composition
- **THEN** nothing is sent

### Requirement: Session identity report
`/peers me` SHALL report this session's own identity instead of a list of others: its bus address,
the bus location, the addresses it subscribes to, its delivery policy, whether presence is
registered, and the senders it trusts. It SHALL report these from the session's live configuration,
and SHALL report the remaining facts even when the bus cannot be read.

#### Scenario: The address
- **WHEN** a human runs `/peers me`
- **THEN** the session's own bus address is shown

#### Scenario: What it listens to
- **WHEN** a human runs `/peers me`
- **THEN** the addresses it subscribes to are shown

#### Scenario: Policy and trust
- **WHEN** a human runs `/peers me`
- **THEN** the delivery policy and the trusted sender set are shown

#### Scenario: Presence
- **WHEN** a human runs `/peers me`
- **THEN** whether presence is registered is shown, and it is shown as off when presence is disabled

#### Scenario: Unreadable bus
- **WHEN** the bus cannot be read
- **THEN** every fact the session already knows is still shown

### Requirement: Peer command
`/peer <address> [message]` SHALL send a message to that address as this session. When the message
is not given it SHALL be composed before sending. When the address is not given, the choice of
peers `/peers` offers SHALL be offered instead. The record SHALL ask the recipient to act on it,
so a peer does not read it as a passing remark.

#### Scenario: Sends to a peer
- **WHEN** a human runs `/peer reviewer "take a look at the parser"`
- **THEN** a record of an asking type is published to `session/reviewer` from this session

#### Scenario: Composed before sending
- **WHEN** a human gives an address but no message
- **THEN** the message is composed first, and nothing is sent if the composition is abandoned

#### Scenario: Chosen instead of typed
- **WHEN** a human gives a message but no address
- **THEN** the peers are offered to choose from and the message is sent to the one chosen

#### Scenario: No message and no address
- **WHEN** a human runs `/peer` with nothing
- **THEN** the peers are offered first, and the message is composed once one is chosen

### Requirement: Reply command
`/reply [message]` SHALL answer the message this session most recently received, addressing its
sender and carrying that message's offset as the reply reference. When no message has been
received there SHALL be nothing to address, and the command SHALL say so rather than sending
anything.

#### Scenario: Answers the latest message
- **WHEN** a human runs `/reply "on it"`
- **THEN** the record is addressed to the sender of the most recently received message

#### Scenario: Carries the reference
- **WHEN** a reply is sent
- **THEN** it carries the offset of the message being answered as its reply reference

#### Scenario: Composed before sending
- **WHEN** a human runs `/reply` with no message
- **THEN** the message is composed first, and nothing is sent if the composition is abandoned

#### Scenario: Nothing to answer
- **WHEN** a human runs `/reply` and no message has been received
- **THEN** the command says there is nothing to reply to and nothing is sent

### Requirement: Command resilience
A failure in a command SHALL NOT crash the session and SHALL NOT leave the bus in a changed state.
A failure SHALL be reported where the session has a UI. A command run where no human can answer a
dialog SHALL report that it needs one rather than failing obscurely.

#### Scenario: The command fails
- **WHEN** a command cannot do what it was asked
- **THEN** the reason is reported and the session continues

#### Scenario: No dialogs available
- **WHEN** a command that needs a dialog runs where none can be shown
- **THEN** it reports that it needs one and sends nothing

#### Scenario: The bus is unreadable
- **WHEN** the bus cannot be read and a command needs it
- **THEN** the failure is reported and consumption and the session are unaffected

### Requirement: Reference annotation
When a delivered record carries a reply reference, the delivered content SHALL identify the record
it refers to by its id, and SHALL quote that record's sender and body when the record can be read.
The lookup SHALL be resolved at delivery and SHALL NOT be remembered between records, and it SHALL
NOT advance any consumer cursor. When the referenced record cannot be read, the annotation SHALL
still name its id rather than failing or omitting it.

#### Scenario: The answer says what it answers
- **WHEN** a record arrives carrying `reply_to` and the referenced record can be read
- **THEN** the delivered content names the referenced id and quotes the record, so the session knows what it is being told about

#### Scenario: Acknowledgements are annotated the same way
- **WHEN** an acknowledgement arrives carrying `reply_to`
- **THEN** the delivered content names the record being acknowledged, so progress and answers are read the same way

#### Scenario: Nothing is remembered
- **WHEN** a record arrives referencing something this session sent before the session started, or from a shell
- **THEN** the reference is still resolved, because nothing depends on what this session recorded earlier

#### Scenario: The reference cannot be read
- **WHEN** the referenced record is no longer in the stream
- **THEN** the delivered content names the id and delivery continues

#### Scenario: Reading is not consuming
- **WHEN** a reference is resolved
- **THEN** no consumer cursor moves, and no record is acknowledged or redelivered as a result

### Requirement: Shipped skill
The extension SHALL make a skill available to every session that loads it, so the model is told how
a process outside the session reaches this one. The skill SHALL be advertised by name and
description without loading its instructions, and loading the extension SHALL NOT depend on the
skill being present or valid.

#### Scenario: The skill is advertised
- **WHEN** a session loads the extension and lists its available skills
- **THEN** the notify skill is offered by name and description

#### Scenario: The instructions load on demand
- **WHEN** the extension is loaded
- **THEN** only the skill's name and description are added, and its instructions are read when a task calls for them

#### Scenario: A broken skill does not break the extension
- **WHEN** the skill directory is missing or malformed
- **THEN** the extension still loads, consumes, and delivers

### Requirement: Presence locator card

A live session SHALL derive a locator card and publish it with its presence when presence is enabled
and locator context is not disabled. The card SHALL contain only `project`, `branch`, `worktree`,
and `path`, with the meanings defined by the presence convention. The session SHALL refresh the
derived card when the working directory changes, at the start of each turn so that a directory
change the host never reported is still picked up before the turn ends, and periodically while it
runs, and SHALL publish a new presence record only when the card actually changes. A failure to derive or publish the card
SHALL NOT stop message consumption or prevent presence without a card.

The session SHALL accept configuration for whether context is derived and for the path mode. The
serialized card SHALL be capped at 512 bytes; a card that would exceed the cap SHALL be omitted while
presence continues.

#### Scenario: Card is published

- **WHEN** a session starts with presence and context derivation enabled in a git repository
- **THEN** its presence record carries the project, branch, worktree, and configured path

#### Scenario: Context disabled

- **WHEN** context derivation is disabled
- **THEN** presence is still registered and no card is published

#### Scenario: Derivation fails

- **WHEN** the locator cannot be derived or the CLI is unavailable
- **THEN** presence is published without a card and consumption continues

#### Scenario: Working directory changes

- **WHEN** the session's working directory changes
- **THEN** the derived card is refreshed and a new presence record is published when it differs

#### Scenario: A turn starts somewhere new

- **WHEN** a turn starts in a directory whose locator differs from the published one
- **THEN** the card is re-derived before the turn's messages are handled and presence is republished

#### Scenario: Oversized card

- **WHEN** a card would serialize to more than 512 bytes
- **THEN** the card is omitted and presence remains registered

### Requirement: Locator card visibility

The agent-facing peer listing SHALL show the same locator column a human sees, so that a peer is
identified by project and branch and not by a session id alone. When the CLI refuses the locator
column as an unknown flag, the listing SHALL fall back to the plain roster rather than fail; a bus
that cannot be read SHALL NOT be retried with another command line.

#### Scenario: The model lists peers

- **WHEN** the model calls the peers tool with the `list` action
- **THEN** each row carries the address, state, remaining lifetime, and locator

#### Scenario: An older CLI

- **WHEN** the CLI reports that it does not know the locator flag
- **THEN** the plain roster is shown instead of an error

### Requirement: Locator card action

The agent-facing peer tool SHALL accept a `card` action whose optional card input is limited to
`project`, `branch`, `worktree`, and `path`. With no card, the action SHALL report the current card.
With a card, it SHALL merge only those keys, re-derive the remaining fields, publish the updated
presence record, and report the result. A null value SHALL remove that override and return the field
to its derived value. The action SHALL NOT accept arbitrary metadata, current state, or work
summaries, and SHALL NOT publish a bus message.

#### Scenario: Read the card

- **WHEN** the model calls the card action without a card
- **THEN** it receives the current locator card

#### Scenario: Override a locator field

- **WHEN** the model calls the card action with a project value
- **THEN** the project override is applied, the other fields are derived, and presence is republished

#### Scenario: Clear an override

- **WHEN** the model calls the card action with a null field
- **THEN** that override is removed, the derived value is used, and presence is republished

#### Scenario: Arbitrary metadata is refused

- **WHEN** the model supplies a key outside the four locator fields
- **THEN** the action refuses it instead of publishing it

### Requirement: An answer that is owed
When a run is about to stop and the question this session was most recently asked, and which asked
for an answer, is still unanswered, the extension SHALL deliver one more entry naming that question
and telling the agent to answer it — or to reply that nothing was needed. It SHALL do this at most
once for that question, SHALL NOT do it once it has been answered, and SHALL NOT do it for a record
that asks for no answer. It SHALL report where the session has a UI when it does it, so that how
often the convention needs rescuing is knowable from use rather than assumed.

#### Scenario: The answer arrives after all
- **WHEN** the agent stops with a question it was asked still unanswered
- **THEN** one entry asks it to answer, and the answer it then sends reaches the asker by the ordinary reply path

#### Scenario: Declining is an answer
- **WHEN** the agent answers that nothing needed reporting
- **THEN** the exchange is finished, and no further entry is delivered for that question

#### Scenario: Once per question
- **WHEN** a question is still unanswered after the agent has been asked about it once
- **THEN** no second entry is delivered for that question
#### Scenario: Already answered
- **WHEN** the agent answered the question while it was working
- **THEN** nothing is delivered when the run stops

#### Scenario: Nothing was asked
- **WHEN** a record that asks for no answer is delivered, or only acknowledgements arrive
- **THEN** no entry is ever delivered on its account

#### Scenario: It says so
- **WHEN** the extension asks the agent for the answer it owes
- **THEN** it reports that it did, where the session has a UI

### Requirement: Completion is reported to the sender
A message the model sends to another session SHALL ask to be told when the run that included it
completes, so that a sender that chose not to wait still learns the peer finished — including when
the peer finished without answering. That report SHALL be delivered as a notification and SHALL NOT
start a turn. It SHALL be additional to the reply, not a substitute for it.

#### Scenario: The peer finishes without replying
- **WHEN** the peer's run that included the message completes and no answer came
- **THEN** the sender is told that the run completed

#### Scenario: It does not wake the sender
- **WHEN** that report arrives at a session that is idle
- **THEN** it is delivered without starting a turn

#### Scenario: The reply is unaffected
- **WHEN** the peer answers
- **THEN** the answer arrives as it always did, and the report neither replaces nor delays it

#### Scenario: A peer that never runs
- **WHEN** the peer never runs the message at all
- **THEN** no report arrives, and a sender that is waiting is bounded only by its own wait window

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

### Requirement: The extension reads through the scoped API

T2.3 ALWAYS, the extension SHALL read a shared stream through a function that takes the credential.
T2.3 ALWAYS, the extension SHALL hold no direct read access to a queue table.

#### Scenario: A shared read goes through the function
- **WHEN** the extension reads a shared stream
- **THEN** the extension SHALL call the read function with the credential

### Requirement: A refused read is reported

T2.4 IF the bus refuses a read, THEN the extension SHALL report the refusal.
T2.4 IF the bus refuses a read, THEN the extension SHALL stop reading that address.

#### Scenario: A refused read is reported
- **WHEN** the extension reads an address outside the scopes of the session
- **THEN** the extension SHALL report the refusal and SHALL deliver nothing

### Requirement: Delivery refuses an unopenable body

T4.1 IF the extension cannot open a body, THEN the extension SHALL report an error naming the key identifier.
T4.1 IF the extension cannot open a body, THEN the extension SHALL start no turn.
T4.1 IF the extension cannot open a body, THEN the extension SHALL publish no acknowledgement.

#### Scenario: The three consequences travel together
- **WHEN** the extension receives a record that the key does not open
- **THEN** the extension SHALL report the error, start no turn, and publish no acknowledgement

### Requirement: A session without a key keeps running

T4.1 ALWAYS, the extension SHALL continue once the extension refuses a body.
T4.1 ALWAYS, the identity report SHALL name the key state.

#### Scenario: The session survives a sealed record
- **WHEN** a session holds no key and receives a sealed record
- **THEN** the session SHALL stay running and SHALL report the key problem
