# Spec Delta

## ADDED Requirements

### Requirement: A record may carry a signature

A record MAY carry a signature in `meta.sig`. The signature SHALL name the algorithm and the public
key that produced it, and SHALL cover the record's canonical sender, its canonical address, and its
body. A record without `meta.sig` SHALL remain a well-formed record.

#### Scenario: A signed record is well formed

- **WHEN** a client publishes a signed record
- **THEN** the record SHALL carry `meta.sig` naming the algorithm, the public key and the signature

#### Scenario: An unsigned record is well formed

- **WHEN** a client publishes a record with no signature
- **THEN** the record SHALL be accepted by the envelope rules exactly as before

#### Scenario: The signature field is additive

- **WHEN** a `v: 1` reader that does not verify meets a signed record
- **THEN** it SHALL preserve `meta.sig` and SHALL read the record

### Requirement: The signed bytes are canonical and fixed

The bytes a signature covers SHALL be a deterministic encoding of the canonical sender, the
canonical address, and the body as stored. Two spellings of the same sender or address SHALL produce
the same signed bytes. The signed bytes SHALL NOT include any field that the bus may rewrite.

#### Scenario: Two spellings sign the same

- **WHEN** the same sender and address are spelled two ways that canonicalize to one form
- **THEN** a signature over one SHALL verify against the other

#### Scenario: A replayed signature does not move

- **WHEN** a signature made for one sender and address is presented with a different sender or
  address
- **THEN** the signature SHALL fail to verify

#### Scenario: A rewritten sender is caught

- **WHEN** a stored record's sender differs from the sender the signature covers
- **THEN** a verifying reader SHALL refuse the record

### Requirement: The signature is readable without the body

A reader SHALL read `meta.sig` without opening the body, and SHALL verify the signature without
requiring the body to be readable as content. A sealed body SHALL be signed as stored bytes.

#### Scenario: A sealed body is verifiable

- **WHEN** a reader holds no key for a sealed body but holds the sender's anchor
- **THEN** the reader SHALL still reach a verdict about the signature

#### Scenario: The envelope stays readable

- **WHEN** a reader that can verify meets a signed record
- **THEN** the sender, the address, the offset and the signature SHALL be readable without the body

### Requirement: An unimplemented algorithm is not trust

A reader that does not implement the algorithm a record names SHALL report the record as unverified
and SHALL NOT report it as trusted or as refused.

#### Scenario: An unknown algorithm is tolerated

- **WHEN** a record names a signature algorithm the reader does not implement
- **THEN** the reader SHALL deliver the record as unverified and consumption SHALL continue

#### Scenario: A new algorithm needs no version bump

- **WHEN** an algorithm is added to the vocabulary
- **THEN** `v` SHALL remain unchanged and readers of the previous version SHALL keep reading

### Requirement: Signed and unsigned records share one stream

A stream MAY hold both signed and unsigned records. A reader SHALL apply its verdict rules per
record, and SHALL NOT treat the presence of unsigned history as making a later signed record
untrusted.

#### Scenario: Mixed history is per record

- **WHEN** a stream holds an unsigned record and a signed record
- **THEN** each SHALL receive its own verdict

#### Scenario: History is never rewritten

- **WHEN** a reader meets unsigned history on a strict bus
- **THEN** the reader SHALL refuse to read it unless the bus permits it, and SHALL rewrite no stored
  record
