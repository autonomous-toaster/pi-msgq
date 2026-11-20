# Spec Delta

## Purpose

How a reader decides whether to believe a sender: the key material and where it lives, the layers a
trust anchor can come from and how they resolve, the verdict a reader reaches about a record, and
what rotation and revocation do to history. This is the reader's opinion about a peer, deliberately
separate from the bus's own authority over who may write.

## ADDED Requirements

### Requirement: The reader verifies, the bus does not

A reader SHALL verify a record's signature against a trust anchor the reader holds. Verification
SHALL NOT depend on a claim made by the bus, and no component of the bus SHALL be consulted for a
trust decision.

#### Scenario: A lying bus cannot forge a sender

- **WHEN** a record's bytes are altered by whatever stores them, or a sender is re-stamped
- **THEN** the reader SHALL reject the record because the signature no longer covers what it stored

#### Scenario: Verification survives an untrusted server

- **WHEN** a reader reads a signed record from a bus operated by somebody else
- **THEN** the reader SHALL reach the same verdict it would reach on a bus it operates

### Requirement: A record has one of three verdicts

For every record a reader SHALL reach exactly one verdict: `trusted`, when the signature verifies
against a current trust anchor; `unverified`, when the record carries no usable signature or names a
key the reader holds no anchor for; or `refused`, when the signature is present and broken or names
a retired key. A refused record SHALL name the reason.

#### Scenario: A trusted record

- **WHEN** a record's signature verifies against the current anchor of the principal that sent it
- **THEN** the reader SHALL report the record as trusted

#### Scenario: A record from a stranger

- **WHEN** a record names a key the reader holds no anchor for
- **THEN** the reader SHALL report the record as unverified and SHALL NOT report it as refused

#### Scenario: A refused record names its reason

- **WHEN** a reader refuses a record
- **THEN** the reported reason SHALL name the principal, the fingerprint, and whether the signature
  was broken or the key was retired

### Requirement: Unverified is a settled state, not a defect

The reader SHALL treat an unverified record as an ordinary record. The reader SHALL NOT infer trust
from repeated contact, from the age of a key, from a bus's own assertion, or from any other evidence
available on the bus. Only an explicit trust act SHALL move a principal out of `unverified`.

#### Scenario: Repetition establishes nothing

- **WHEN** a reader receives a hundred records signed by one key the reader holds no anchor for
- **THEN** every one of them SHALL remain unverified

#### Scenario: Trust is only ever granted by an act

- **WHEN** a principal becomes trusted
- **THEN** some human-initiated trust act SHALL have recorded the anchor before the fact

### Requirement: Trust is scoped to one bus

A trust anchor SHALL apply to exactly one bus, identified stably across invocations and distinctly
from other buses. A reader connected to a bus SHALL consult only that bus's anchors. An anchor
recorded for one bus SHALL NOT apply to another.

#### Scenario: A shared hub does not leak into a private bus

- **WHEN** an anchor is recorded for a principal on a shared server bus
- **THEN** records from that principal on a local file bus SHALL remain unverified

#### Scenario: The bus is named where the anchors are shown

- **WHEN** a reader shows the anchors in force
- **THEN** the report SHALL name the bus those anchors apply to

### Requirement: Anchors come from three layers with a fixed precedence

A trust anchor SHALL be obtainable from the environment for one invocation, from the current
session, or from a durable per-bus keyring. Where more than one layer holds an anchor for one
principal, the more transient layer SHALL take precedence, in the order environment, then session,
then durable.

#### Scenario: An invocation may trust a peer without recording it

- **WHEN** a caller names an anchor in the environment for one invocation
- **THEN** that anchor SHALL apply to that invocation and SHALL NOT be written anywhere

#### Scenario: A session may trust a peer for its own duration

- **WHEN** an anchor is recorded for the current session only
- **THEN** it SHALL apply while the session lives and SHALL NOT survive it

#### Scenario: The durable layer is the default

- **WHEN** an anchor is recorded with no more transient layer named
- **THEN** it SHALL be written to the per-bus keyring and SHALL apply to later invocations

### Requirement: Disagreeing layers are reported, never reconciled

Where two layers hold different anchors for one principal, the reader SHALL use the layer the
precedence rule selects and SHALL report the disagreement, naming both fingerprints and both
layers.

#### Scenario: A stale durable anchor is named

- **WHEN** the session holds one fingerprint for a principal and the durable keyring holds another
- **THEN** the reader SHALL use the session anchor and SHALL report that the two disagree

#### Scenario: Agreement is silent

- **WHEN** two layers hold the same fingerprint for a principal
- **THEN** the reader SHALL report no disagreement

### Requirement: Rotation retires a key and refuses its records

Rotating a principal's anchor SHALL move the previous fingerprint to a retired state in the same
trust act. A record signed by a retired key SHALL be refused rather than trusted, and rotation
SHALL report how many stored records the rotation makes refused.

#### Scenario: Old records stop being trusted

- **WHEN** a principal's key is rotated
- **THEN** a record signed by the previous key SHALL be refused

#### Scenario: Rotation states its cost

- **WHEN** an operator rotates an anchor
- **THEN** the report SHALL name how many stored records become refused

#### Scenario: A retired key is distinguishable from a stranger

- **WHEN** a record is signed by a retired key rather than an unknown one
- **THEN** the reader SHALL refuse it rather than call it unverified

### Requirement: A key belongs to one principal

A trust anchor SHALL name one principal and one public key. One key SHALL NOT be trusted for two
principals, and a principal SHALL hold one current key.

#### Scenario: Reusing a key elsewhere is refused

- **WHEN** an operator records a key already anchored to another principal
- **THEN** the trust act SHALL be refused and SHALL name the principal that already holds it

#### Scenario: One current key per principal

- **WHEN** a principal holds a current anchor and another is recorded
- **THEN** the operation SHALL be a rotation, not a second anchor

### Requirement: Only public material is ever recorded

A trust act SHALL record a public key and a fingerprint. It SHALL NOT write, transmit, or report
private key material, and no part of the trust layer SHALL send private material over a bus.

#### Scenario: The keyring holds no secret

- **WHEN** an operator reads the keyring of a bus
- **THEN** it SHALL hold no private key

#### Scenario: A bus cannot ask to be trusted

- **WHEN** a record arrives asking to be trusted, or carrying a key for the reader to adopt
- **THEN** the reader SHALL record nothing and SHALL leave the sender unverified
