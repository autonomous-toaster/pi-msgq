# Spec Delta

## ADDED Requirements

### Requirement: A publish seals to one party, always

A publish to an address that names one party SHALL seal the body to that party and to the sender. A publish to a party whose agreement key is unknown SHALL fail rather than store the body unsealed, and there SHALL be no way to ask for plaintext. The bus roster address SHALL be the one exception.

#### Scenario: A session address is sealed
- **WHEN** a client publishes to a `session/` or `cli/` address and holds the recipient's agreement key
- **THEN** the body SHALL be sealed to that recipient and to the sender

#### Scenario: An unknown recipient stops the publish
- **WHEN** a client publishes to a party whose key it does not hold
- **THEN** the publish SHALL fail, naming the address, and nothing SHALL be stored

#### Scenario: The roster is the exception
- **WHEN** a client publishes a presence record to the `bus` roster address
- **THEN** the body SHALL NOT be sealed, because every participant reads it

#### Scenario: No plaintext escape
- **WHEN** a caller looks for a way to store a body unsealed for a party
- **THEN** there SHALL be none, and the flag that used to exist SHALL be a usage error

## REMOVED Requirements

### Requirement: A publish seals to a recipient by rule
**Reason**: Replaced by "A publish seals to one party, always". The earlier rule described three
exceptions that no longer exist: a group keeping the bus key, a first contact falling back, and
`--plaintext`.
**Migration**: Publish to a party whose key is known; a peer advertises its key on its presence record,
so an unknown key means nobody is there. Bodies stored unsealed for a party under the old rule stay
readable as plaintext.
