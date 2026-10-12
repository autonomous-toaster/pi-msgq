## ADDED Requirements

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

## MODIFIED Requirements

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
