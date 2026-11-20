# msgq-encryption Specification

## Purpose
How a body is sealed and opened, how a client obtains a key, and what a client does with a body that
the client cannot open.

## Requirements

### Requirement: A reader opens a body

T2.2 IF a record names the reader as a recipient, THEN the reader SHALL open the body.
T2.2 IF a record is sealed and does not name the reader, THEN the reader SHALL report that it is sealed to somebody else and SHALL NOT treat that as an error.
T2.2 IF the body of a record that names the reader fails to open, THEN the reader SHALL report an error naming the key.
T2.2 IF the body fails to open, THEN the reader SHALL treat no byte of the body as content.

#### Scenario: A tampered body is refused
- **WHEN** a reader opens a body that an editor altered
- **THEN** the reader SHALL report an error naming the key

#### Scenario: Somebody else's record is not an error
- **WHEN** a reader meets a record sealed to another recipient
- **THEN** the reader SHALL report who it is sealed to, and the read SHALL be a success

#### Scenario: An algorithm this build lacks is not an error
- **WHEN** a reader meets a body encoded with an algorithm it does not implement
- **THEN** the reader SHALL report the algorithm and SHALL NOT fail the read

### Requirement: The key state is reported

T3.3 ALWAYS, db info SHALL report the one sealing scheme in force.
T3.3 ALWAYS, db info SHALL name the agreement key this session would seal with.
T3.3 ALWAYS, db info SHALL report no bus key, because there is none.
T3.3 ALWAYS, db info SHALL print no key material.

#### Scenario: The report names the key
- **WHEN** a client runs db info with a key configured
- **THEN** the report SHALL name the agreement key that key seals with, and SHALL print no key material

#### Scenario: The report names the mode
- **WHEN** a client runs db info
- **THEN** the report SHALL name the recipient scheme as the only way a body is sealed

### Requirement: A key is found on the record itself

A sender's agreement key SHALL travel inside the bytes its signature covers, so a reader obtains it
from any record that sender wrote. A key SHALL NOT be fetched from a directory, a card, or any surface
that expires.

#### Scenario: A key from a past record
- **WHEN** a client seals to an address whose sender has written at least one signed record
- **THEN** the client SHALL obtain the agreement key from that record, without a new lookup

#### Scenario: A key from nobody
- **WHEN** a client is asked to seal to an address it has no key for
- **THEN** the client SHALL refuse and name the address, rather than store the body unsealed

#### Scenario: A substituted key fails the signature
- **WHEN** a record's agreement key is altered on the bus
- **THEN** the record SHALL be refused, because the signature covers that key

### Requirement: A group is sealed with the bus key and says so

T2.1 IF a record's address names a group, THEN sealing to recipients SHALL NOT apply.
T2.1 ALWAYS, a client asked to seal to a group SHALL use the bus key when one is configured, and
SHALL report what it used.

#### Scenario: A group is not a recipient
- **WHEN** a client publishes to a group address
- **THEN** the record SHALL be sealed under the bus key if one is configured, and the client SHALL say
  which of the two happened

#### Scenario: Naming a recipient for a group is honoured
- **WHEN** a client names an explicit recipient for a group address
- **THEN** the record SHALL be sealed to that recipient, because the caller said who reads it

### Requirement: Rotation archives what it replaces

IF a principal's key is replaced, THEN the client SHALL keep the key it replaces, because a record
sealed under it remains sealed under it forever. A retired decryption key SHALL NOT be deleted, and
SHALL NOT be published.

#### Scenario: Old ciphertext stays readable after rotation
- **WHEN** a principal rotates their key and later reads a record sealed under the previous key
- **THEN** the client SHALL open it with the archived key

#### Scenario: An archived key is never published
- **WHEN** a key is archived
- **THEN** no part of it SHALL leave the machine

### Requirement: A key is a principal's own

T1.1 ALWAYS, a key SHALL be derived from the seed a principal signs with, or agreed for one record.
T1.1 ALWAYS, the bus SHALL store no key material beside the bus file.
T1.1 ALWAYS, no passphrase SHALL be consulted for sealing or opening.

#### Scenario: The seed is the root
- **WHEN** a client seals or opens a body
- **THEN** every key it uses SHALL be derived from the principal's seed or agreed for that record

#### Scenario: No key material is written down
- **WHEN** the bus stores a sealed body
- **THEN** the bus file SHALL hold no key material

#### Scenario: There is no second source of keys
- **WHEN** a passphrase is present in the environment
- **THEN** it SHALL have no effect on what is sealed or how, and the client SHALL NOT read it

### Requirement: A body is sealed to one party, always

T2.1 IF a record is published to one party, THEN its body SHALL be sealed to that party and to its sender.
T2.1 ALWAYS, the body SHALL be sealed under a fresh content key, and that key delivered to each recipient separately under a key agreed with that recipient, with a fresh ephemeral per delivery.
T2.1 ALWAYS, the record envelope SHALL stay readable.
T2.1 ALWAYS, a fresh nonce SHALL accompany every sealed body.
T2.1 ALWAYS, an unsealed body SHALL be refused for any address that names a party.

#### Scenario: Two seals differ
- **WHEN** a client seals one body twice
- **THEN** the two sealed bodies SHALL differ

#### Scenario: The envelope stays readable
- **WHEN** a client reads a sealed record without the key
- **THEN** the address, the sender and the offset SHALL stay readable

#### Scenario: One body, several readers
- **WHEN** a record is sealed to more than one recipient
- **THEN** the body SHALL be sealed once and each recipient SHALL receive only the content key

#### Scenario: An ephemeral is not shared
- **WHEN** one record is sealed to several recipients
- **THEN** each delivery of the content key SHALL use its own ephemeral key

#### Scenario: There is no unsealed path to a party
- **WHEN** a client publishes to a party it holds no agreement key for
- **THEN** the publish SHALL fail, and nothing SHALL be stored
