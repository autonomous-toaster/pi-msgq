# Spec Delta

## ADDED Requirements

### Requirement: The extension reads through the scoped API

T2.3 ALWAYS, the extension SHALL read a shared stream through a function that takes the credential.
T2.3 ALWAYS, the extension SHALL hold no direct read access to a queue table.

#### Scenario: A shared read goes through the function
- **WHEN** the extension reads a shared stream
- **THEN** the extension SHALL call the read function with the credential

### Requirement: A refused read is reported

T2.4 IF the bus refuses a read, THEN the extension SHALL report the refusal.
T2.4 IF the bus refuses a read, THEN the extension SHALL stop reading that address.

#### Scenario: A refused read is reported
- **WHEN** the extension reads an address outside the scopes of the session
- **THEN** the extension SHALL report the refusal and SHALL deliver nothing
