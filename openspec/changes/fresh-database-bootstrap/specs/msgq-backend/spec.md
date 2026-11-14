# Spec Delta

## ADDED Requirements

### Requirement: An empty database bootstraps in one connection

T1.1 ALWAYS, a first connection to an empty database SHALL create the bus schema.

#### Scenario: A database with nothing in it
- **WHEN** a client runs any command against a database with no schema
- **THEN** the command SHALL succeed and the schema SHALL exist afterwards

#### Scenario: A published record lands in a fresh database
- **WHEN** a client publishes to an address on a database the client has just bootstrapped
- **THEN** a subsequent read SHALL return the record

### Requirement: Tables exist before a block reads them

T1.1 BEFORE a block reads a table, the bus SHALL create that table. T1.1, T3.1

#### Scenario: A block that reads identity
- **WHEN** the schema file runs against an empty database
- **THEN** the table the block reads SHALL already exist

### Requirement: A bus with no identity is readable

T2.1 IF no identity exists, THEN a read SHALL proceed without a credential. T2.1, T2.2

#### Scenario: The unauthenticated bus
- **WHEN** a client reads an address on a bus with no identity registered
- **THEN** the read SHALL succeed and SHALL list the addresses of that bus

### Requirement: A read is refused once an identity exists

T2.1 IF an identity exists, THEN the bus SHALL refuse a read that presents no resolvable credential. T2.2

#### Scenario: The door closes with the first identity
- **WHEN** a client reads an address with no credential on a bus that holds one identity
- **THEN** the bus SHALL refuse the read
