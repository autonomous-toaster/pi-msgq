# Spec Delta

## ADDED Requirements

### Requirement: Trusting a peer is a command, not a tool

The extension SHALL expose trust through a user-initiated command and SHALL NOT expose it as a tool
the model can call. A record's content SHALL NOT cause a trust anchor to be written.

#### Scenario: The model cannot pin a peer

- **WHEN** the model asks to trust a sender, whatever the reason
- **THEN** no anchor SHALL be written

#### Scenario: The operator pins a peer

- **WHEN** a person runs the trust command with a principal and a fingerprint
- **THEN** the extension SHALL record the anchor and report it

#### Scenario: A message cannot request trust

- **WHEN** a delivered record asks to be trusted or carries a key for adoption
- **THEN** the extension SHALL record nothing and SHALL leave the sender unverified

### Requirement: The trust command reports where each anchor lives

The trust command SHALL show the bus it applies to, the durable anchors of that bus, the anchors of
the current session, and any principal whose layers disagree. Recording an anchor SHALL default to
the durable layer, and SHALL write to the session layer only when the caller asks for it.

#### Scenario: Status separates the layers

- **WHEN** a person runs the trust command with nothing to record
- **THEN** it SHALL report the bus, the durable anchors, the session anchors and any disagreement

#### Scenario: Default is durable

- **WHEN** a person records an anchor without naming a layer
- **THEN** the anchor SHALL persist for later sessions on that bus

#### Scenario: A session anchor dies with the session

- **WHEN** a person records an anchor for the current session only
- **THEN** it SHALL apply while the session lives and SHALL NOT be reported by a later session

### Requirement: The extension consults one bus

The extension SHALL resolve trust against the bus it is connected to and SHALL NOT consult anchors
recorded for any other bus. The bus SHALL be fixed for the life of the session.

#### Scenario: A hub anchor does not apply locally

- **WHEN** the session is connected to a local file bus
- **THEN** anchors recorded for a shared server bus SHALL NOT make any sender trusted

#### Scenario: The report names its bus

- **WHEN** the trust command reports anchors
- **THEN** it SHALL name the bus those anchors belong to

### Requirement: Every delivered record shows its verdict

The extension SHALL render each record with its verdict — trusted, unverified or refused — and
SHALL NOT hide an unverified record. A refused record SHALL be shown as refused with its reason
rather than dropped silently.

#### Scenario: A stranger is shown as unverified

- **WHEN** a record arrives signed by a key the session holds no anchor for
- **THEN** it SHALL be delivered and marked unverified

#### Scenario: A broken signature is shown as refused

- **WHEN** a record's signature does not verify
- **THEN** it SHALL be shown as refused with the reason and SHALL NOT be delivered as ordinary
  content

#### Scenario: A retired key is not a stranger

- **WHEN** a record is signed by a key the current keyring retires
- **THEN** it SHALL be shown as refused for the retirement, not as unverified

### Requirement: The session counts whom it has not pinned

The extension SHALL report, in the trust command's status, how many records delivered in the session
came from unverified senders, so that an operator sees the peers they have not pinned.

#### Scenario: Unpinned peers are counted

- **WHEN** a session has delivered records from senders with no anchor
- **THEN** the status SHALL name how many records those senders accounted for
