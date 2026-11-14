# Spec Delta

## ADDED Requirements

### Requirement: Publish seals the body

T3.1 IF a key is configured, THEN the CLI SHALL seal the body on publish.
T3.1 IF no key is configured, THEN the CLI SHALL store a plaintext body.
T3.1 IF no key is configured, THEN the CLI SHALL report that the body is plaintext.

#### Scenario: A configured key seals
- **WHEN** an operator publishes with a key configured
- **THEN** the stored body SHALL be sealed

### Requirement: Read opens the body

T3.2 IF the body opens, THEN the CLI SHALL print the content.
T3.2 IF the body fails to open, THEN the CLI SHALL exit non-zero.
T3.2 IF the body fails to open, THEN the CLI SHALL print no body.
T3.2 IF the body fails to open, THEN the CLI SHALL name the key identifier.

#### Scenario: An unopenable body is an error
- **WHEN** an operator reads a sealed record without the key
- **THEN** the CLI SHALL exit non-zero and SHALL print no body
