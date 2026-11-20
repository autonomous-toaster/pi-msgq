# msgq-encryption Specification

## Purpose
How a body is sealed and opened, how a client obtains a key, and what a client does with a body that
the client cannot open.

## Requirements

### Requirement: A key comes from custody

T1.1 ALWAYS, the bus SHALL derive the key from a principal's seed when one is available.
T1.1 ALWAYS, the bus SHALL derive a further key from a passphrase when no seed applies.
T1.1 ALWAYS, the bus SHALL store no key material beside the bus file.
T1.1 ALWAYS, a derived key SHALL be separate from every other key derived from the same seed.

#### Scenario: The passphrase is asked for
- **WHEN** a client seals or opens a body
- **THEN** the client SHALL obtain the passphrase from the operator or from a configured source

#### Scenario: No key material is written down
- **WHEN** the bus stores a sealed body
- **THEN** the bus file SHALL hold no key material

#### Scenario: The seed is the root
- **WHEN** a client seals or opens a body and the principal has a key
- **THEN** the keys it uses SHALL be derived from that seed, and the seed SHALL NOT be used directly

#### Scenario: The passphrase still applies
- **WHEN** a client seals a body for an address that names no recipient
- **THEN** the client SHALL use the passphrase-derived key when one is configured

### Requirement: A body is sealed

T2.1 IF a record names recipients, THEN publish SHALL seal its body under a fresh content key.
T2.1 ALWAYS, the content key SHALL be delivered to each recipient separately, under a key agreed with
that recipient.
T2.1 ALWAYS, a fresh ephemeral key SHALL accompany every delivery of the content key.
T2.1 ALWAYS, the record envelope SHALL stay readable.
T2.1 ALWAYS, a fresh nonce SHALL accompany every sealed body.

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

### Requirement: A reader opens a body

T2.2 IF a record names the reader as a recipient, THEN the reader SHALL open the body.
T2.2 IF a record is sealed and does not name the reader, THEN the reader SHALL report that it is sealed
to somebody else and SHALL NOT treat that as an error.
T2.2 IF the body of a record that names the reader fails to open, THEN the reader SHALL report an error
naming the key.
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

### Requirement: A mixed history stays readable

T2.3 ALWAYS, a reader SHALL read a plaintext record without a key.
T2.3 ALWAYS, a reader SHALL read a sealed record that its key opens.
T2.3 ALWAYS, a reader SHALL read a record sealed before this version under the rules it was sealed
with, including its additional data and its key identifier.
T2.3 ALWAYS, the bus SHALL preserve a sealed record that the reader cannot open.

#### Scenario: One stream holds both kinds
- **WHEN** a stream holds a plaintext record and a sealed record
- **THEN** the reader SHALL read both

#### Scenario: An older seal still opens
- **WHEN** a reader meets a record sealed under the previous scheme
- **THEN** the reader SHALL open it exactly as it did before this version

### Requirement: The key state is reported

T3.3 ALWAYS, db info SHALL report which sealing mode this invocation would use.
T3.3 ALWAYS, db info SHALL name the recipients a record would be sealed to.
T3.3 ALWAYS, db info SHALL print no key material.

#### Scenario: The report names the key
- **WHEN** a client runs db info with a key configured
- **THEN** the report SHALL name the key identifier and SHALL print no key material

#### Scenario: The report names the mode
- **WHEN** a client runs db info with a key configured
- **THEN** the report SHALL name the sealing mode and the agreement key in force

### Requirement: The stored bytes carry no plaintext

T5.2 ALWAYS, the bus file SHALL hold no plaintext body.
T5.2 ALWAYS, the engine SHALL hold no plaintext body.

#### Scenario: A search of the file finds nothing
- **WHEN** an operator searches the bus file for a known message body
- **THEN** the search SHALL find no match

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
