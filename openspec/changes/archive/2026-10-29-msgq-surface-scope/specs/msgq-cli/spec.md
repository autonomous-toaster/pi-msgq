## MODIFIED Requirements

### Requirement: Sessions listing
`msgq sessions` SHALL list the live participants recorded as unexpired presence records in the bus.
Each entry SHALL show the participant's address, kind, state, and remaining lifetime. Records older
than the participant's declared lifetime SHALL be excluded, at most one entry SHALL be shown per
participant, and `--json` SHALL emit one JSON object per participant. The listing SHALL be derived from
records alone, so that any participant able to publish is listed, whether or not it is a pi session.

#### Scenario: Lists a live session
- **WHEN** a participant has published a presence record that has not yet expired
- **THEN** `msgq sessions` shows its address and its state

#### Scenario: Any participant can appear
- **WHEN** a process publishes a presence record using nothing but the command-line client
- **THEN** it is listed alongside pi sessions

#### Scenario: Excludes expired leases
- **WHEN** a presence record is older than the lifetime it declares
- **THEN** that participant is not listed

#### Scenario: Retraction is immediate
- **WHEN** a participant retracts its presence before shutting down
- **THEN** it is not listed, without waiting for the lifetime to elapse

#### Scenario: One entry per participant
- **WHEN** more than one presence record exists for the same participant
- **THEN** the listing shows a single entry for it, reflecting the most active state and the newest record

#### Scenario: Kind is reported
- **WHEN** participants of more than one kind are present
- **THEN** each entry reports its kind

#### Scenario: JSON output
- **WHEN** `--json` is given
- **THEN** each participant is emitted as one JSON object per line, with the same fields as before

#### Scenario: Nobody online
- **WHEN** no presence record is live
- **THEN** the command succeeds and lists nothing

### Requirement: Database info and protocol metadata
`msgq db info` SHALL report the resolved path, existence, size, per-stream message counts, consumer
counts, the bus protocol version, and the health of the bus: for each consumer, how far behind the
head of its streams it is, and for each addressed stream, the age of its oldest record. It SHALL
report the age of the presence address separately, because that address carries every session's
churn. The bus SHALL record `protocol_version` and `created_by` in a table it owns and SHALL NOT
modify the engine's internal tables on the write path.

#### Scenario: New database records protocol version
- **WHEN** the database is created
- **THEN** protocol version 1 is recorded

#### Scenario: Info reports provenance
- **WHEN** `msgq db info` runs
- **THEN** it reports the protocol version and the resolved file path

#### Scenario: Info answers whether anything is stuck
- **WHEN** `msgq db info` runs on a bus in use
- **THEN** it reports each consumer's lag and each stream's oldest-record age, so a stalled participant
  is visible without reading the streams

## ADDED Requirements

### Requirement: Retention

`msgq prune` SHALL remove records older than a stated age, and SHALL refuse to remove a record that
any consumer has not yet passed, so that pruning can never destroy work that is still owed. It SHALL
report what it would remove before doing it unless explicitly told to proceed, and SHALL preserve the
most recent presence record of every address regardless of age.

#### Scenario: Old records go
- **WHEN** `msgq prune --older-than` names an age and records are older than it
- **THEN** those records are removed and the database shrinks

#### Scenario: Pinned cursors are respected
- **WHEN** a consumer's cursor sits before a record that would otherwise be pruned
- **THEN** that record is kept, and the reason is reported

#### Scenario: Presence survives
- **WHEN** every presence record for an address is older than the stated age
- **THEN** the most recent one is kept, so the participant still ages out on its own terms

#### Scenario: Nothing is removed unasked
- **WHEN** `msgq prune` runs without being told to proceed
- **THEN** it reports what it would remove and removes nothing

### Requirement: Cursor collection

Cursors belonging to participants that are no longer live SHALL be removable, so that a bus used for
months does not accumulate one cursor per address per dead session.

#### Scenario: Dead participant's cursors go
- **WHEN** cursors are collected and the participant has no live presence record
- **THEN** its cursors are removed

#### Scenario: Live participants are untouched
- **WHEN** cursors are collected and the participant is live
- **THEN** its cursors are kept, including the one that would otherwise look idle

#### Scenario: Collecting is reported
- **WHEN** cursors are collected
- **THEN** the number removed is reported
