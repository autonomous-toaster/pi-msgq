# msgq-identity Specification

## Purpose
Who may speak on the bus and as whom: the rules that let a shared database host several users without
turning the sender field into an unverifiable claim, while leaving a single-owner bus as trusting as
the permissions on the bus file.

## Requirements

### Requirement: Identity is a principal

T1.1 ALWAYS, one identity row SHALL name one principal.
T1.1 ALWAYS, the name of an identity row SHALL hold a principal name rather than an address.
T1.1 ALWAYS, the scopes of an identity row SHALL hold the addresses that the principal may reach.
T1.1 ALWAYS, a scope SHALL cover the address that the scope names.

#### Scenario: One row per user
- **WHEN** an operator registers a principal
- **THEN** the bus SHALL store one identity row for that principal

#### Scenario: A scope covers a subtree
- **WHEN** a scope names `group/red`
- **THEN** the scope SHALL cover `group/red/sub` and SHALL NOT cover `group/redder`

### Requirement: The bus stamps the sender

T1.2 ALWAYS, the bus SHALL store the sender named by the record once the scopes of the writer cover the sender.
T1.2 ALWAYS, the bus SHALL store no sender other than the sender named by the record.

#### Scenario: The sender is the writer
- **WHEN** a client publishes a record naming a sender inside the scopes of the client
- **THEN** the stored record SHALL carry that sender

#### Scenario: Reading reports the stamped sender
- **WHEN** a reader reads a record written under authorization
- **THEN** the record's sender SHALL be the sender that the bus stored

### Requirement: A claim outside the scopes is refused

T1.2 IF a record names a sender outside the scopes of the writer, THEN the bus SHALL refuse the write.
T1.2 IF the bus refuses a write, THEN the bus SHALL store no record.

#### Scenario: A claim outside the namespace is refused
- **WHEN** a client publishes a record naming a sender outside the scopes of the client
- **THEN** the bus SHALL refuse the write and SHALL store no record

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

### Requirement: Reads follow the scopes

T2.1 ALWAYS, a read SHALL name an address inside the scopes of the caller.
T2.1 ALWAYS, the bus SHALL refuse a read of an address outside the scopes of the caller.
T2.1 ALWAYS, a catalog listing SHALL name only the addresses inside the scopes of the caller.

#### Scenario: A cross-tenant read is refused
- **WHEN** a client reads an address outside the scopes of the client
- **THEN** the bus SHALL refuse the read

#### Scenario: A catalog lists one tenant
- **WHEN** a client lists addresses
- **THEN** the listing SHALL name only the addresses inside the scopes of the client

### Requirement: The writing principal is recorded

T4.1 ALWAYS, the bus SHALL stamp the writing principal in the headers of a stored record.
T4.1 ALWAYS, a reader SHALL learn the writing principal from the headers.

#### Scenario: The headers name the principal
- **WHEN** a client reads a record written under authorization
- **THEN** the headers SHALL name the principal that wrote the record

### Requirement: A credential lease is renewable

T5.1 ALWAYS, the bus SHALL accept a renewal that extends an existing identity row.
T5.1 ALWAYS, a renewal SHALL preserve the credential digest.
T5.1 ALWAYS, the bus SHALL accept a renewal once the lease elapsed.
T5.1 ALWAYS, a session SHALL renew the credential at session start.

#### Scenario: A hard expiry refuses a renewal
- **WHEN** a principal holds a hard expiry that passed
- **THEN** the bus SHALL refuse the renewal

#### Scenario: A renewal keeps sibling sessions working
- **WHEN** one session renews the credential of a principal
- **THEN** every other session of that principal SHALL keep writing with the same credential

#### Scenario: A resumed session renews once the lease lapsed
- **WHEN** a session starts once the lease of the credential elapsed
- **THEN** the session SHALL renew the credential and SHALL continue writing

### Requirement: A room is a shared scope

T6.1 ALWAYS, a room SHALL be an address listed in the scopes of more than one principal.
T6.1 ALWAYS, prune authority over an address SHALL follow ownership of the address rather than read access to the address.

#### Scenario: Two principals share a room
- **WHEN** two principals list one address in the scopes of both principals
- **THEN** both principals SHALL reach that address

#### Scenario: A member cannot prune a shared room
- **WHEN** a principal without ownership of a shared room prunes that room
- **THEN** the bus SHALL refuse the prune
