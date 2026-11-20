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
T1.1 ALWAYS, a scope SHALL cover exactly the address it names.

#### Scenario: One row per user
- **WHEN** an operator registers a principal
- **THEN** the bus SHALL store one identity row for that principal

#### Scenario: A scope covers a subtree
- **WHEN** a scope names an address
- **THEN** the scope SHALL cover that address and SHALL NOT cover a name that merely shares its prefix

#### Scenario: A scope covers one stream
- **WHEN** a scope names `session/alice-1`
- **THEN** it SHALL cover that stream and SHALL NOT cover `session/alice-2`

#### Scenario: No scheme names a set
- **WHEN** an operator writes a scope
- **THEN** the scope SHALL name one party's address, because no scheme addresses a set of them any more

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

### Requirement: The stamp is authorization, not proof

The sender the bus stores SHALL be read as a statement that the writer was authorized to use that
address at write time. A reader SHALL NOT treat the stored sender as proof of authorship unless a
signature verifies against an anchor the reader holds.

#### Scenario: An authorized record is not yet a proven record

- **WHEN** a record is written under authorization with no signature
- **THEN** the stored sender SHALL be read as an authorization and SHALL NOT be reported as proof of
  authorship

#### Scenario: A scope claim is not authorship

- **WHEN** a principal's credential covers an address and a record names that address as its sender
- **THEN** the reader SHALL learn only that the writer was permitted to use the address

### Requirement: One credential names one principal, not one session

The bus SHALL distinguish principals and SHALL NOT distinguish the sessions of one principal. Two
sessions presenting one credential SHALL be indistinguishable to the bus, and neither SHALL be
prevented from naming an address inside the shared scopes.

#### Scenario: Sibling sessions are indistinguishable to the bus

- **WHEN** two sessions of one principal present the same credential
- **THEN** the bus SHALL record each as the same principal

#### Scenario: A sibling may name a sibling's address

- **WHEN** a session names a participant address inside its principal's scopes, including an address
  another session of that principal uses
- **THEN** the bus SHALL accept the write, because scope is what the bus checks

#### Scenario: Telling siblings apart needs a signature

- **WHEN** a reader must attribute a record to one session rather than to a principal
- **THEN** the reader SHALL rely on a signature, not on the stored sender

### Requirement: A signature adds no authority

A signature SHALL NOT widen what the bus permits. A signed record naming an address outside the
scopes of the writer's principal SHALL be refused by the bus exactly as an unsigned one is.

#### Scenario: Signing does not bypass scope

- **WHEN** a client publishes a signed record naming an address outside the scopes of its principal
- **THEN** the bus SHALL refuse the write and SHALL store no record

#### Scenario: The two checks are independent

- **WHEN** a record passes the scope check and carries a broken signature
- **THEN** the bus SHALL store it and a reader SHALL refuse it for the signature
