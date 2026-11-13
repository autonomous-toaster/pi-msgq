## MODIFIED Requirements

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
