## ADDED Requirements

### Requirement: Human command surface
The extension SHALL register commands a human runs from the prompt without the model: `/peers`,
`/peer`, `/reply`, and `/companions`. Each SHALL appear in the session's command list with a
description of what it does. A command SHALL act with the session's own identity on the same bus
the model's tools use, and the records it publishes SHALL be ordinary records: the same address
rules, the same types, and the same reply reference as any other publish. A command SHALL return
without waiting for a reply, because a reply is an inbound message and arrives by the path that
already delivers one.

#### Scenario: Commands are discoverable
- **WHEN** a human lists the session's commands
- **THEN** `/peers`, `/peer`, `/reply`, and `/companions` are listed with a description

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

### Requirement: Companions command
`/companions` SHALL report the companions this session can see, each with its name, bus address,
herdr status, and working directory. Where herdr is unavailable the command SHALL report that
rather than failing, and the other commands SHALL remain usable.

#### Scenario: The companions
- **WHEN** a human runs `/companions`
- **THEN** each running companion is shown with its name, address, herdr status, and working directory

#### Scenario: Richer than the roster
- **WHEN** a companion is listed
- **THEN** its herdr status and working directory are shown, which the bus roster does not carry

#### Scenario: Outside herdr
- **WHEN** herdr is unavailable
- **THEN** the command reports that, and `/peers`, `/peer`, and `/reply` still work

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

## MODIFIED Requirements

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
