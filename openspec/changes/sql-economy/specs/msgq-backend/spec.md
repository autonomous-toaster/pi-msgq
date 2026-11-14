# Spec Delta

## ADDED Requirements

### Requirement: The bootstrap runs once per schema version

T1.2 ALWAYS, the bus SHALL record the installed schema version in `msgq.meta`.
T1.2 ALWAYS, a connection SHALL install the schema only when the recorded version differs from the version of the running build.

#### Scenario: A second command skips the schema
- **WHEN** a client runs a second command against a bus whose schema version matches the build
- **THEN** the command SHALL install no schema and SHALL write no catalogue change

### Requirement: A newer bus is refused

T1.2 IF the recorded version is newer than the version of the running build, THEN the bus SHALL refuse the connection. T1.2

#### Scenario: A newer bus is refused
- **WHEN** a client connects to a bus whose schema version is newer than the client build
- **THEN** the client SHALL exit with an error naming both versions

### Requirement: The catalogue answers without counts

T1.3 ALWAYS, the catalogue SHALL accept a request for addresses without record counts.

#### Scenario: The catalogue is asked for no counts
- **WHEN** a caller lists addresses and asks for no counts
- **THEN** the catalogue SHALL return each address and the head of each address

### Requirement: A catalogue request without counts returns no count

T1.3 IF a caller requests no counts, THEN the catalogue SHALL return the address and the head alone. T2.1, T2.5

#### Scenario: A listing that does not count
- **WHEN** a caller lists addresses without counts on an address that holds many records
- **THEN** the listing SHALL cost the same as on an address that holds none
