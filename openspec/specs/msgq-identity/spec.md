# msgq-identity Specification

## Purpose
Who may speak on the bus and as whom: the rules that let a shared database host several users without
turning the sender field into an unverifiable claim, while leaving a single-owner bus as trusting as
the permissions on the bus file.

## Requirements

### Requirement: The bus stamps the sender

T5.2 BEFORE the bus stores a record under authorization, the bus SHALL derive the stored sender from the writer's identity.
T5.2 BEFORE the bus stores a record under authorization, the bus SHALL discard any supplied sender value.

#### Scenario: The sender is the writer
- **WHEN** a client publishes a record claiming a sender the client does not own
- **THEN** the stored record SHALL carry the sender derived from the client's identity

#### Scenario: Reading reports the stamped sender
- **WHEN** a reader reads a record written under authorization
- **THEN** the record's sender SHALL be the address the bus stamped

### Requirement: The bus refuses a foreign sender

T5.2 IF a writer names another user's address, THEN the bus SHALL store the writer's own address.

#### Scenario: A foreign sender is replaced
- **WHEN** a writer names another user's address
- **THEN** the bus SHALL store the writer's own address

### Requirement: A record carries no credential

T5.1 ALWAYS, a stored record SHALL contain no credential.
T5.1 ALWAYS, a reader of the stream SHALL obtain no write capability from history.

#### Scenario: History reveals no credential
- **WHEN** a reader reads back any record written under authorization
- **THEN** the record SHALL contain no credential

#### Scenario: History reveals no derivation
- **WHEN** a reader reads back any record written under authorization
- **THEN** the record SHALL contain no value that derives a credential

### Requirement: The credential travels out of band

T5.1 ALWAYS, a client SHALL present the credential on the connection.
T5.1 ALWAYS, a client SHALL present no credential inside a record.

#### Scenario: The transport carries the secret
- **WHEN** a client authenticates to a backend with authorization
- **THEN** the credential SHALL travel on the connection

### Requirement: Scope allows a write

T5.2 IF an address is inside an identity's write scopes, THEN the bus SHALL store the record.

#### Scenario: Own namespace is writable
- **WHEN** a client writes to an address inside the client's scopes
- **THEN** the bus SHALL store the record

### Requirement: Scope refuses a write

T5.2 IF an address is outside an identity's write scopes, THEN the bus SHALL refuse the write.
T5.2 IF an address is outside an identity's write scopes, THEN the bus SHALL store nothing.
T5.2 IF the address spelling varies, THEN the bus SHALL check the canonical address.

#### Scenario: Another namespace is refused
- **WHEN** a client writes to an address outside the client's scopes
- **THEN** the bus SHALL refuse the write

#### Scenario: Scope cannot be escaped by spelling
- **WHEN** a client writes to a differently-spelled form of an address outside the client's scopes
- **THEN** the bus SHALL refuse the write

### Requirement: One write path

T5.3 ALWAYS, the authorizing write path SHALL be the only write path for the runtime role.

#### Scenario: A direct write is refused
- **WHEN** the runtime role attempts a direct write to a stream
- **THEN** the bus SHALL refuse the write

### Requirement: The local engine needs no credential

T3.2 IF the active backend cannot authorize, THEN the bus SHALL require no credential.

#### Scenario: Local bus needs no credential
- **WHEN** a client publishes to a local file bus with no credential configured
- **THEN** the bus SHALL write the record

### Requirement: The trust statement is stated

T3.2 ALWAYS, the local engine SHALL state that the sender is a claim.
T2.6 ALWAYS, the documentation SHALL state the trust statement of each engine.

#### Scenario: No false assurance is claimed
- **WHEN** the local bus describes the trust model
- **THEN** the description SHALL state that the sender is a claim

### Requirement: A rejected credential is an error

T5.4 IF the bus finds no credential, THEN the bus SHALL report an error naming the reason.
T5.4 IF a credential expires, THEN the bus SHALL report an error naming the reason.
T5.4 IF the bus rejects a credential, THEN the bus SHALL report an error naming the reason.

#### Scenario: Expired credential is an error
- **WHEN** a client attempts a write with an expired credential
- **THEN** the write SHALL fail with an error naming the expiry

#### Scenario: Missing credential is an error
- **WHEN** a client attempts a write with no credential
- **THEN** the write SHALL fail with an error naming the absence

### Requirement: No downgrade

T5.4 ALWAYS, a rejected credential SHALL cause no downgrade to an unauthenticated write.

#### Scenario: No silent downgrade
- **WHEN** the bus rejects a credential
- **THEN** the bus SHALL write no record

### Requirement: The failure is visible

T6.4 ALWAYS, a session without write authority SHALL be distinguishable from an idle session.

#### Scenario: The failure is visible
- **WHEN** the write of a session fails because of the session credential
- **THEN** the reported state SHALL name the credential problem
