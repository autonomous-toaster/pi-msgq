# msgq-encryption Specification

## Purpose
How a body is sealed and opened, how a client obtains a key, and what a client does with a body that
the client cannot open.

## Requirements

### Requirement: A key comes from custody

T1.1 ALWAYS, the bus SHALL derive the key from a passphrase.
T1.1 ALWAYS, the bus SHALL store no key material beside the bus file.
T1.1 ALWAYS, the bus SHALL derive a key identifier from the key.

#### Scenario: The passphrase is asked for
- **WHEN** a client seals or opens a body
- **THEN** the client SHALL obtain the passphrase from the operator or from a configured source

#### Scenario: No key material is written down
- **WHEN** the bus stores a sealed body
- **THEN** the bus file SHALL hold no key material

### Requirement: A body is sealed

T2.1 ALWAYS, publish SHALL seal the body under the key.
T2.1 ALWAYS, the sealed body SHALL carry the algorithm and the key identifier in the record metadata.
T2.1 ALWAYS, the record envelope SHALL stay readable.
T2.1 ALWAYS, a fresh nonce SHALL accompany every sealed body.

#### Scenario: Two seals differ
- **WHEN** a client seals one body twice
- **THEN** the two sealed bodies SHALL differ

#### Scenario: The envelope stays readable
- **WHEN** a client reads a sealed record without the key
- **THEN** the address, the sender and the offset SHALL stay readable

### Requirement: A reader opens a body

T2.2 IF the key identifier matches a key that the reader holds, THEN the reader SHALL open the body.
T2.2 IF the body fails to open, THEN the reader SHALL report an error naming the key identifier.
T2.2 IF the body fails to open, THEN the reader SHALL treat no byte of the body as content.

#### Scenario: A tampered body is refused
- **WHEN** a reader opens a body that an editor altered
- **THEN** the reader SHALL report an error naming the key identifier

### Requirement: A mixed history stays readable

T2.3 ALWAYS, a reader SHALL read a plaintext record without a key.
T2.3 ALWAYS, a reader SHALL read a sealed record that the key opens.
T2.3 ALWAYS, the bus SHALL preserve a sealed record that the reader cannot open.

#### Scenario: One stream holds both kinds
- **WHEN** a stream holds a plaintext record and a sealed record
- **THEN** the reader SHALL read both

### Requirement: The key state is reported

T3.3 ALWAYS, db info SHALL report the key state.
T3.3 ALWAYS, db info SHALL name the key identifier in use.
T3.3 ALWAYS, db info SHALL print no key material.

#### Scenario: The report names the key
- **WHEN** a client runs db info with a key configured
- **THEN** the report SHALL name the key identifier and SHALL print no key material

### Requirement: The stored bytes carry no plaintext

T5.2 ALWAYS, the bus file SHALL hold no plaintext body.
T5.2 ALWAYS, the engine SHALL hold no plaintext body.

#### Scenario: A search of the file finds nothing
- **WHEN** an operator searches the bus file for a known message body
- **THEN** the search SHALL find no match
