# Spec Delta

## MODIFIED Requirements

### Requirement: Additional subscriptions
The extension SHALL accept additional addresses from configuration, as a comma-separated list
supplied by both a command-line flag and an environment variable. Each additional address SHALL
be subscribed independently, and an invalid address SHALL disable the extension with a clear
message rather than being ignored.

#### Scenario: Extra group topics
- **WHEN** configuration lists `session/backend` and `session/ci`
- **THEN** the extension subscribes to both in addition to its own address

#### Scenario: Invalid address
- **WHEN** configuration contains an address with an unknown scheme
- **THEN** the extension reports the error and does not start consuming

#### Scenario: A scheme that no longer exists
- **WHEN** configuration lists `group/backend`
- **THEN** the extension reports that the scheme is gone, names the ones that exist, and does not start
  consuming

## ADDED Requirements

### Requirement: Presence goes to the bus roster and is never sealed

The extension SHALL publish its presence to `bus/presence`, or `bus/<scope>/presence` when a scope is
configured, and SHALL leave that record unsealed. The roster is public metadata that every participant
reads, and a client with no key must still be able to find the participants and their agreement keys.

#### Scenario: Presence is published to the roster address
- **WHEN** a session registers or renews its presence
- **THEN** the record SHALL be written to the `bus` roster address for its scope

#### Scenario: Presence is never sealed
- **WHEN** key material is configured for the session
- **THEN** the presence record SHALL still be stored unsealed, and its signature SHALL carry the
  session's agreement key

#### Scenario: The roster is readable without a key
- **WHEN** a session with no key of its own reads the roster
- **THEN** it SHALL read every presence record and learn each participant's agreement key
