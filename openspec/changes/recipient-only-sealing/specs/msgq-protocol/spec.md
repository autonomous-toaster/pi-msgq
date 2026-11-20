# Spec Delta

## MODIFIED Requirements

### Requirement: A body may declare its encoding

T1.3 ALWAYS, an encoded body SHALL carry a `meta.enc` declaration.
T1.3 ALWAYS, a client SHALL preserve an unrecognized `meta.enc` key.
T1.3 ALWAYS, a declaration SHALL name the algorithm, the additional data it was sealed under, and the parties who can open it.
T6.3 ALWAYS, a reader SHALL treat an unhonourable `meta.enc` as an opaque body.

#### Scenario: Declaration travels in meta
- **WHEN** a sender encodes a body for a reader
- **THEN** `meta.enc` SHALL name the encoding and the parties who can open it

#### Scenario: Ignoring the declaration is harmless
- **WHEN** a client reads a record with a `meta.enc` value that the client does not understand
- **THEN** the client SHALL preserve the key and SHALL NOT fail

#### Scenario: The declaration names who can open it
- **WHEN** a body is sealed to recipients
- **THEN** the declaration SHALL name, for each of them, the fingerprint of the key that opens it, and
  SHALL name no key material beyond the ephemeral public keys

#### Scenario: One binding
- **WHEN** a body is sealed
- **THEN** its additional data SHALL bind the canonical sender and the address, and there SHALL be no
  older binding to accept

### Requirement: An encoded body is declared

T2.1 ALWAYS, a sealed body SHALL declare an algorithm and the parties who can open it in the record metadata.
T2.1 ALWAYS, a sealed body MAY declare more than one recipient, each with its own ephemeral key.
T2.1 ALWAYS, the record envelope SHALL stay readable.
T2.1 ALWAYS, a presence body SHALL stay readable.
T2.1 ALWAYS, an acknowledgement body SHALL stay readable.

#### Scenario: A reader that knows no encoding
- **WHEN** a reader meets a declaration that the reader does not recognize
- **THEN** the reader SHALL preserve the record and SHALL treat the body as opaque

#### Scenario: A recipient list is additive
- **WHEN** a `v: 1` reader that predates this convention meets a record sealed to recipients
- **THEN** it SHALL preserve the record and treat the body as opaque, exactly as for any encoding it
  does not implement

### Requirement: A reader preserves what the reader cannot open

T2.3 ALWAYS, a reader SHALL preserve a record that the reader cannot open.
T2.3 ALWAYS, a reader SHALL rewrite no stored record.

#### Scenario: A sealed record survives a read
- **WHEN** a client reads a sealed record without the key
- **THEN** the stored record SHALL stay unchanged

#### Scenario: The bus file holds no plaintext body
- **WHEN** an operator searches the bus file for a known message body that was sealed
- **THEN** the search SHALL find no match
