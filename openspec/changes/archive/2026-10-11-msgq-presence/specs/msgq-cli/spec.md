## ADDED Requirements

### Requirement: Sessions listing
`msgq sessions` SHALL list the live participants recorded as unexpired leases in the bus. Each
entry SHALL show the participant's address, kind, state, and remaining lifetime. Expired leases
SHALL be excluded, at most one entry SHALL be shown per participant, and `--json` SHALL emit one
JSON object per participant.

#### Scenario: Lists a live session
- **WHEN** a session holds an unexpired lease
- **THEN** `msgq sessions` shows its address and its state

#### Scenario: Excludes expired leases
- **WHEN** a lease has expired
- **THEN** that participant is not listed

#### Scenario: One entry per participant
- **WHEN** more than one lease row exists for the same participant
- **THEN** the listing shows a single entry for it

#### Scenario: Kind is reported
- **WHEN** participants of more than one kind are present
- **THEN** each entry reports its kind

#### Scenario: JSON output
- **WHEN** `--json` is given
- **THEN** each participant is emitted as one JSON object per line

#### Scenario: Nobody online
- **WHEN** no lease is live
- **THEN** the command succeeds and lists nothing
