# Spec Delta

## ADDED Requirements

### Requirement: One anchor covers both purposes

The fingerprint a person compares SHALL certify every key a principal uses, because the further keys
travel inside the bytes the signing key covers. A trust act SHALL therefore need nothing beyond the one
fingerprint it already records.

#### Scenario: Pinning covers the agreement key
- **WHEN** a reader pins a principal at a fingerprint
- **THEN** the agreement keys that principal advertises SHALL be trusted to the same degree, with no
  second act

#### Scenario: A key that does not chain up is unverified
- **WHEN** a record advertises an agreement key under a signature that does not verify against an anchor
- **THEN** the record SHALL be unverified or refused as the signature decides, and the agreement key
  SHALL be treated no better than the record that carried it
