# Spec Delta

## MODIFIED Requirements

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

## ADDED Requirements

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

## REMOVED Requirements

### Requirement: A key comes from custody
**Reason**: Replaced by "A key is a principal's own": a passphrase-derived bus key no longer exists, so
there is no custody to ask for and no key identifier to print.
**Migration**: Unset `MSGQ_PASSPHRASE`, `MSGQ_PASSPHRASE_FILE` and `MSGQ_KEY_SALT`; they are not read.

### Requirement: A body is sealed
**Reason**: Replaced by "A body is sealed to one party, always", which states the recipient rule as the
only rule rather than as a condition on top of the bus key.
**Migration**: Republish what matters. Bodies sealed under the removed scheme are not readable.

### Requirement: A mixed history stays readable
**Reason**: Only one scheme produces sealed bodies now, so there is no mix to read.
**Migration**: Records sealed under the removed scheme remain in the bus and remain unreadable; prune
them like any other record.

### Requirement: The stored bytes carry no plaintext
**Reason**: The property belongs to the record, not to one key scheme, and is stated in `msgq-protocol`
under "A reader preserves what the reader cannot open".
**Migration**: None; the property still holds and is still tested.
