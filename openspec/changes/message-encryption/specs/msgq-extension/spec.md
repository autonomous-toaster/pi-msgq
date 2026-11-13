# Spec Delta

## ADDED Requirements

### Requirement: Delivery refuses an unopenable body

T4.1 IF the extension cannot open a body, THEN the extension SHALL report an error naming the key identifier.
T4.1 IF the extension cannot open a body, THEN the extension SHALL start no turn.
T4.1 IF the extension cannot open a body, THEN the extension SHALL publish no acknowledgement.

#### Scenario: The three consequences travel together
- **WHEN** the extension receives a record that the key does not open
- **THEN** the extension SHALL report the error, start no turn, and publish no acknowledgement

### Requirement: A session without a key keeps running

T4.1 ALWAYS, the extension SHALL continue once the extension refuses a body.
T4.1 ALWAYS, the identity report SHALL name the key state.

#### Scenario: The session survives a sealed record
- **WHEN** a session holds no key and receives a sealed record
- **THEN** the session SHALL stay running and SHALL report the key problem
