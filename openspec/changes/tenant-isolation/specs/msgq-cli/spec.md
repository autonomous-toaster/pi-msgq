# Spec Delta

## ADDED Requirements

### Requirement: Cursors and consumers stay inside the caller's namespace

T3.4 ALWAYS, the consumers listing SHALL name only the consumers owned by the caller.
T3.4 ALWAYS, a cursor command SHALL act on a cursor row owned by the caller.
T3.4 ALWAYS, the consumers listing SHALL show the owning principal.

#### Scenario: Another tenant's consumer is absent
- **WHEN** a consumer of another principal holds a cursor on an address
- **THEN** the listing SHALL NOT name that consumer

### Requirement: Prune reports the removal the caller may perform

T3.5 ALWAYS, prune SHALL remove records only from an address inside the scopes of the caller.
T3.5 ALWAYS, prune SHALL report the total removed.
T3.5 ALWAYS, a refused prune SHALL leave the bus unchanged.

#### Scenario: A refused prune changes nothing
- **WHEN** prune names an address outside the scopes of the caller
- **THEN** the CLI SHALL exit non-zero and SHALL remove no record

### Requirement: The identity report names the caller

T4.2 ALWAYS, db info SHALL report the principal of the caller.
T4.2 ALWAYS, db info SHALL report the scopes of the caller.

#### Scenario: The report names the principal
- **WHEN** a client runs db info against a shared engine
- **THEN** the report SHALL name the principal and the scopes of the client
