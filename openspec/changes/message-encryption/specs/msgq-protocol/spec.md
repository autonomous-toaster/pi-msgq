# Spec Delta

## ADDED Requirements

### Requirement: An encoded body is declared

T2.1 ALWAYS, a sealed body SHALL declare an algorithm and a key identifier in the record metadata.
T2.1 ALWAYS, the record envelope SHALL stay readable.
T2.1 ALWAYS, a presence body SHALL stay readable.
T2.1 ALWAYS, an acknowledgement body SHALL stay readable.

#### Scenario: A reader that knows no encoding
- **WHEN** a reader meets a declaration that the reader does not recognize
- **THEN** the reader SHALL preserve the record and SHALL treat the body as opaque

### Requirement: A reader preserves what the reader cannot open

T2.3 ALWAYS, a reader SHALL preserve a record that the reader cannot open.
T2.3 ALWAYS, a reader SHALL rewrite no stored record.

#### Scenario: A sealed record survives a read
- **WHEN** a client reads a sealed record without the key
- **THEN** the stored record SHALL stay unchanged

### Requirement: Fixtures cover an encoded body

T5.1 ALWAYS, the fixtures SHALL hold a sealed body.
T5.1 ALWAYS, the fixtures SHALL hold a tampered body.
T5.1 ALWAYS, the fixtures SHALL hold one stream with a plaintext body and a sealed body.

#### Scenario: Both clients read the fixtures
- **WHEN** either client runs the fixtures
- **THEN** both clients SHALL agree on every encoded case
