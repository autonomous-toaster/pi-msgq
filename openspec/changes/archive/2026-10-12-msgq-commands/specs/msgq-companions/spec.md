## ADDED Requirements

### Requirement: Listing companions
`companion list` SHALL report the companions that are running: every named agent herdr reports
that is also a participant on the bus, matched by the name shared between the herdr agent, the pi
session, and the bus address. Each entry SHALL carry the name, the bus address, the herdr status,
and the working directory. The command SHALL report where herdr is unavailable rather than failing,
and SHALL be able to emit its result as one JSON object.

#### Scenario: Lists what is running
- **WHEN** companions are running
- **THEN** each is listed with its name, bus address, herdr status, and working directory

#### Scenario: Richer than the bus roster
- **WHEN** a companion is listed
- **THEN** its herdr status and working directory are reported, which presence alone does not carry

#### Scenario: Not a herdr agent
- **WHEN** a bus participant has no herdr agent of that name
- **THEN** it is reported as a peer but not as a companion

#### Scenario: Machine readable
- **WHEN** JSON output is requested
- **THEN** the result is one JSON object whose entries carry the same fields

#### Scenario: Outside herdr
- **WHEN** herdr is unavailable
- **THEN** the command reports that rather than failing destructively
