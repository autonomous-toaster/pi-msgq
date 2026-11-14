# Spec Delta

## Purpose

Who a caller is, what a caller may reach, and how a claimed sender is resolved: the rules that let one
database host several users without any user reaching another user's state.

## MODIFIED Requirements

### Requirement: The bus stamps the sender

T1.2 ALWAYS, the bus SHALL store the sender named by the record once the scopes of the writer cover the sender.
T1.2 ALWAYS, the bus SHALL store no sender other than the sender named by the record.

#### Scenario: The sender is the writer
- **WHEN** a client publishes a record naming a sender inside the scopes of the client
- **THEN** the stored record SHALL carry that sender

#### Scenario: Reading reports the stamped sender
- **WHEN** a reader reads a record written under authorization
- **THEN** the record's sender SHALL be the sender that the bus stored

## REMOVED Requirements

### Requirement: The bus refuses a foreign sender

**Reason**: T1.2 ALWAYS, the bus SHALL refuse a sender claim outside the scopes of the writer, so no address remains for a rewrite.

**Migration**: T1.2 ALWAYS, a client SHALL name a sender inside the scopes of the client, or SHALL receive a refusal and store nothing.

## ADDED Requirements

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

### Requirement: A claim outside the scopes is refused

T1.2 IF a record names a sender outside the scopes of the writer, THEN the bus SHALL refuse the write.
T1.2 IF the bus refuses a write, THEN the bus SHALL store no record.

#### Scenario: A claim outside the namespace is refused
- **WHEN** a client publishes a record naming a sender outside the scopes of the client
- **THEN** the bus SHALL refuse the write and SHALL store no record

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
