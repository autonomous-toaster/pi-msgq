# Spec Delta

## ADDED Requirements

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
