## ADDED Requirements

### Requirement: Command delivery
When a record of type `command` arrives from a sender the session trusts, the extension SHALL
deliver its body as **input** rather than as content — dispatched exactly as if the user had typed
it, so that a prompt template expands, a skill loads, an extension command runs, and anything
unrecognized passes through as ordinary text. Because a command must be the whole text, the
extension SHALL send a note naming the sender and the address the record arrived on immediately
before it. A command SHALL NOT be collected into a coalesced batch, since a batch is delivered as a
single message and the command would stop being one. From a sender outside the trusted set a command
SHALL be delivered display-only, like any other turn-bearing record.

#### Scenario: The command runs
- **WHEN** a trusted sender publishes a command naming a prompt template
- **THEN** the session runs that template, as if it had been typed

#### Scenario: Provenance survives
- **WHEN** a command is delivered
- **THEN** an entry naming the sender and the receiving address precedes it in the transcript

#### Scenario: A command is never coalesced
- **WHEN** a command arrives while the agent is running
- **THEN** it is queued for delivery rather than gathered into a batch

#### Scenario: An untrusted command
- **WHEN** a command arrives from a sender outside the trusted set
- **THEN** it is delivered display-only and nothing runs

#### Scenario: Nothing to run
- **WHEN** a command names something this session does not have
- **THEN** its text is delivered as an ordinary prompt rather than failing

## MODIFIED Requirements

### Requirement: Peer tool
The extension SHALL expose a tool that lets the model work with other agents on the bus. It SHALL
be able to list the reachable peers, send a message, send a command for a peer to run, ask a
question and receive the answer, and reply to the message it most recently received. Sending SHALL
go through the bus and SHALL NOT require the peer to have been started by this session.

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

#### Scenario: Sending a command
- **WHEN** the model sends a command to a peer
- **THEN** the record's type is `command` and its body is the command line, so the peer runs it rather than reading about it

#### Scenario: Asking a command is refused
- **WHEN** the model tries to ask a peer a question that is also a command
- **THEN** the tool refuses, and says that the request for an answer belongs in the command's own arguments
