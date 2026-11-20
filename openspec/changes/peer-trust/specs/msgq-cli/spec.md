# Spec Delta

## MODIFIED Requirements

### Requirement: Output modes and exit codes

Commands SHALL print human-readable output by default. `--json` SHALL print one JSON object per
line. A command that prints a stored record SHALL print the same object every other such command
prints: the topic, the offset, the recorded time, the verdict, and the record. The exit status SHALL
be 0 on success, 1 on a runtime or database error, 2 on a usage error, and 3 when an await finishes
without a terminal acknowledgement.

#### Scenario: JSON output

- **WHEN** `--json` is provided to a command that prints a stored record
- **THEN** each record is emitted as one JSON object per line, carrying the record and its verdict

#### Scenario: Usage error

- **WHEN** an unknown flag is provided
- **THEN** the CLI exits with status 2

#### Scenario: Await timeout

- **WHEN** an await finishes without a terminal acknowledgement
- **THEN** the CLI exits with status 3

## ADDED Requirements

### Requirement: A key is generated and named by a fingerprint

The client SHALL create a principal's signing key on request, SHALL store it where a private key
belongs, and SHALL report its public key and fingerprint in a form a peer can copy and compare.
Generating a key SHALL NOT publish anything and SHALL NOT contact a bus.

#### Scenario: A key is made without a bus

- **WHEN** an operator generates a key with no bus configured
- **THEN** the client SHALL write the private key, report the public key and the fingerprint, and
  publish nothing

#### Scenario: The fingerprint is the comparable form

- **WHEN** an operator asks for the fingerprint of a key
- **THEN** the client SHALL report a value short enough to read aloud and unambiguous enough to
  distinguish two keys

#### Scenario: The private key is not printed

- **WHEN** any command reports a key
- **THEN** it SHALL print no private key material

### Requirement: Trust is recorded by an explicit verb

The client SHALL offer one verb that records a trust anchor for a principal, one that revokes it,
one that rotates it, and one that lists what is trusted. Recording an anchor SHALL require a
principal and a fingerprint given by the caller, SHALL NOT adopt a key seen on a bus, and SHALL NOT
run as part of any other command.

#### Scenario: A peer is pinned from a fingerprint

- **WHEN** an operator records that a principal is trusted at a fingerprint
- **THEN** the client SHALL write the anchor to the keyring of the current bus and report it

#### Scenario: An unrelated command never trusts

- **WHEN** a client publishes, reads, follows or lists without an explicit trust verb
- **THEN** no anchor SHALL be written

#### Scenario: Rotation states what it breaks

- **WHEN** an operator rotates a principal's anchor
- **THEN** the client SHALL report how many stored records become refused

#### Scenario: Listing names the bus and both layers

- **WHEN** an operator lists what is trusted
- **THEN** the report SHALL name the bus, the durable anchors, the anchors of this invocation, and
  any disagreement between layers

### Requirement: Trust may come from the environment for one invocation

The client SHALL read trust anchors from an environment variable holding one or more
`principal:fingerprint` entries. A malformed entry SHALL be a fatal error. Two entries for one
principal with different fingerprints SHALL be a fatal error. An anchor read from the environment
SHALL NOT be written to any file.

#### Scenario: Several peers are trusted at once

- **WHEN** the variable holds more than one entry
- **THEN** every entry SHALL apply to that invocation

#### Scenario: A malformed entry stops the run

- **WHEN** an entry does not hold a principal and a fingerprint
- **THEN** the client SHALL fail with an error naming the entry and SHALL trust nothing from it

#### Scenario: A contradictory entry stops the run

- **WHEN** the variable names one principal twice with two different fingerprints
- **THEN** the client SHALL fail rather than choose between them

#### Scenario: The environment trust is visible afterwards

- **WHEN** an operator asks the client about the bus
- **THEN** the anchors supplied by the environment SHALL be reported as originating there

### Requirement: A bus refuses unsigned records unless it is told otherwise

By default a client SHALL refuse to read an unsigned record and SHALL name the setting that permits
it. A single environment setting SHALL permit reading unsigned records for that invocation only.
The permission SHALL be reported wherever the trust posture is reported.

#### Scenario: Unsigned history is refused by default

- **WHEN** a client reads a stream holding an unsigned record and nothing permits it
- **THEN** the read SHALL fail with an error naming the offending record and the setting

#### Scenario: The escape hatch is per invocation

- **WHEN** the setting permits unsigned records for one invocation
- **THEN** the next invocation SHALL be strict again

#### Scenario: Leniency is reported

- **WHEN** an operator asks the client about the bus while the setting is in force
- **THEN** the report SHALL state that unsigned records are being read

### Requirement: A record is printed with its position and its verdict

Every command that prints a whole stored record SHALL print one shape: the topic, the offset, the
recorded time, the verdict, and the record itself as the bus stored it. The verdict SHALL NOT be
added to the record. A command that prints no whole record — an acknowledgement summary, a listing,
a report — SHALL be unchanged.

#### Scenario: One shape for a record

- **WHEN** a whole record is printed by a read, a follow or an ask
- **THEN** the output SHALL carry the topic, the offset, the recorded time, the verdict and the record

#### Scenario: The record stays the record

- **WHEN** a verdict accompanies a record
- **THEN** the record SHALL be exactly the bytes the bus stored, with no field added

#### Scenario: Text names the verdict too

- **WHEN** a record is printed in the human-readable form
- **THEN** the verdict SHALL be visible on the line

### Requirement: The verdict is the same on every path

The verdict a reader reaches SHALL come from one implementation, whether it is computed during a
read or asked for separately. A client other than the command-line one SHALL be able to obtain the
verdict for a record it read by itself.

#### Scenario: A separate client asks

- **WHEN** a client hands one or more records to the verification surface
- **THEN** it SHALL receive one verdict per record, in the order the records were given

#### Scenario: The two paths agree

- **WHEN** the same record is read by the command-line client and handed to the verification surface
- **THEN** both SHALL report the same verdict

#### Scenario: A malformed record is not a crash

- **WHEN** a record that is not a JSON object is handed to the verification surface
- **THEN** it SHALL report that record as unverified with the reason, and SHALL continue with the rest

### Requirement: The trust posture is reportable

The client's description of the bus SHALL report the trust posture: whether unsigned records are
refused, which trust layers are in force, and the identity of the bus the anchors belong to. A read
SHALL report how many of the records it showed came from senders the reader holds no anchor for.

#### Scenario: The posture is named

- **WHEN** an operator asks about the bus
- **THEN** the report SHALL name the trust posture and the layers in force

#### Scenario: The bus is identified

- **WHEN** an operator asks about the bus
- **THEN** the report SHALL name the bus its anchors apply to

#### Scenario: Unverified senders are counted where the records are seen

- **WHEN** a read shows records from senders with no anchor
- **THEN** that read SHALL say how many of them there were
