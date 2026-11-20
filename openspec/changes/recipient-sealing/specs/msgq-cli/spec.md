# Spec Delta

## ADDED Requirements

### Requirement: A publish seals to a recipient by rule

A publish to an address that names one participant SHALL seal the body to that participant and to the
sender. A publish to a group SHALL NOT. A publish that cannot reach the recipient's key SHALL fail
rather than store the body unsealed, unless the caller asks for plaintext explicitly.

#### Scenario: A session address is sealed
- **WHEN** a client publishes to `session/` or `cli/` and holds the recipient's agreement key
- **THEN** the body SHALL be sealed to that recipient and to the sender

#### Scenario: A group address keeps the bus key
- **WHEN** a client publishes to a group address
- **THEN** the body SHALL be sealed under the bus key when one is configured, and the client SHALL say
  which of the two happened

#### Scenario: An unknown recipient stops the publish
- **WHEN** a client publishes to a participant whose key it does not hold
- **THEN** the publish SHALL fail, naming the address and the flag that would store it unsealed

#### Scenario: Plaintext has to be meant
- **WHEN** the caller asks for plaintext explicitly
- **THEN** the body SHALL be stored unsealed and the client SHALL say so

### Requirement: Opening is one verb

The client SHALL open sealed records on request, reading records from standard input and reporting one
outcome per record: the opened record, the recipient it is sealed to, or the algorithm it cannot
honour. A client other than the command-line one SHALL obtain opened records from this verb rather than
implement the encoding again.

#### Scenario: A record opens
- **WHEN** a client hands over a record sealed to it
- **THEN** it SHALL receive the record with its body opened

#### Scenario: Somebody else's record
- **WHEN** a client hands over a record sealed to another recipient
- **THEN** it SHALL be told who it is sealed to, and the command SHALL succeed

#### Scenario: A tampered body
- **WHEN** a client hands over a record sealed to it whose body was altered
- **THEN** it SHALL be told the body does not open, naming the key

#### Scenario: One implementation
- **WHEN** another client needs to open records
- **THEN** it SHALL ask this verb, so that one implementation decides what an encoding is

### Requirement: A replaced key is archived

IF a key is replaced, THEN the client SHALL keep the key it replaces, SHALL report where it was kept,
and SHALL NOT delete it while records sealed under it remain readable.

#### Scenario: Rotation keeps the old key
- **WHEN** an operator replaces this machine's key
- **THEN** the client SHALL archive the replaced key and name the file

#### Scenario: The archive is local
- **WHEN** a key is archived
- **THEN** nothing about it SHALL be published to a bus
