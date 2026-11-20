# Spec Delta

## MODIFIED Requirements

### Requirement: Presence is a convention over event records

T5.5 ALWAYS, presence SHALL be an `event` record with `event.kind` of `presence`.
T5.5 ALWAYS, a presence record SHALL carry a `state` and a lifetime.
T5.5 ALWAYS, presence SHALL travel to one well-known address per scope, under the `bus` scheme.
T5.5 ALWAYS, a presence record SHALL NOT be sealed: it is public metadata that every participant reads.
T5.5 ALWAYS, a participant SHALL count as live while the newest presence record is younger than the declared lifetime.
T5.5 ALWAYS, the bus SHALL judge liveness by recorded time.
T5.5 ALWAYS, a roster SHALL list only the participants of the roster scope.
T5.5 ALWAYS, a clean shutdown SHALL publish a presence record that retracts the participant.

#### Scenario: Any client can appear
- **WHEN** a process publishes a presence record with nothing but the command-line client
- **THEN** the roster SHALL list the process alongside pi sessions

#### Scenario: Retraction is immediate
- **WHEN** a participant retracts presence on clean shutdown
- **THEN** the roster SHALL drop the participant without waiting for the lifetime

#### Scenario: Crash ages out
- **WHEN** a participant stops without retracting presence
- **THEN** the roster SHALL drop the participant after the declared lifetime elapses

#### Scenario: Scopes do not see each other
- **WHEN** two participants publish presence in different scopes
- **THEN** each roster SHALL list only the participants of one scope

#### Scenario: The roster is readable by everybody
- **WHEN** a client with no key of its own reads the presence address
- **THEN** the record SHALL be readable and SHALL NOT be sealed, and its signature SHALL carry the
  participant's agreement key
