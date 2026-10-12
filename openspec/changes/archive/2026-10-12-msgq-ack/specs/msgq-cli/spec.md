## ADDED Requirements

### Requirement: Awaiting acknowledgements
`msgq send --await <address> [message]` SHALL publish the message and then wait for
acknowledgements of it. `msgq await <offset>` SHALL wait for acknowledgements of an offset that was
already published. Waiting SHALL begin from a position recorded before publishing, so an
acknowledgement that arrives early is still reported. Each acknowledgement SHALL be reported as it
arrives, and the command SHALL finish when a terminal acknowledgement is seen. When no address is
given, the wait SHALL address itself to the sender recorded on the message at that offset, so the
caller does not have to know where the acknowledgements will land. `msgq await --json`
SHALL emit one JSON object per acknowledgement; `msgq send --await` prints human-readable phases,
because `--json` on `publish` already selects JSON record input.

#### Scenario: Phases are reported as they arrive
- **WHEN** an awaited message is started and then completed by its session
- **THEN** the command reports the `started` and `completed` acknowledgements

#### Scenario: An early acknowledgement is not missed
- **WHEN** an acknowledgement arrives between publishing and the wait beginning
- **THEN** it is still reported

#### Scenario: Reply address is derived
- **WHEN** no address is given, and the message at that offset was sent by a known address
- **THEN** the wait follows that address

#### Scenario: Terminal phase ends the wait
- **WHEN** a terminal acknowledgement arrives
- **THEN** the command finishes successfully

#### Scenario: Display delivery ends the wait
- **WHEN** the awaited message was delivered display-only
- **THEN** the wait finishes at its `received` acknowledgement

#### Scenario: Timeout
- **WHEN** no terminal acknowledgement arrives within the wait window
- **THEN** the command stops waiting and reports that it timed out

#### Scenario: Unbounded wait
- **WHEN** the wait window is set to zero
- **THEN** the command waits without a limit

#### Scenario: JSON output
- **WHEN** `msgq await --json` is given
- **THEN** each acknowledgement is emitted as one JSON object per line

### Requirement: Acknowledgement rendering
Human-readable output SHALL render an acknowledgement as a lifecycle line rather than a raw JSON
blob, naming it as an acknowledgement and showing the offset it refers to, its phase, and any
delivery or outcome detail. JSON output SHALL remain the raw stored record.

#### Scenario: Lifecycle line
- **WHEN** an acknowledgement is printed in human-readable output
- **THEN** the line names it as an acknowledgement and shows the referenced offset, the phase, and any detail

#### Scenario: JSON unchanged
- **WHEN** `--json` is given
- **THEN** the acknowledgement is printed as its raw stored record

## MODIFIED Requirements

### Requirement: Publish
`msgq publish <address> [message]` SHALL append one message to the stream named by the canonical address and print the assigned offset to stdout. It SHALL be able to request acknowledgements on the record it publishes.

#### Scenario: Successful publish
- **WHEN** a message is published
- **THEN** the assigned offset is printed and the message is returned by a subsequent read or follow

#### Scenario: Default sender
- **WHEN** `--from` is omitted
- **THEN** the record's `from` is `cli/<user>@<host>`

#### Scenario: Body from stdin
- **WHEN** the message argument is `-`
- **THEN** the body is read from stdin

#### Scenario: Requesting an acknowledgement
- **WHEN** `--ack` is given
- **THEN** the published record requests acknowledgement of every phase, or of the phases named when a list is given

### Requirement: Output modes and exit codes
Commands SHALL print human-readable output by default and raw stored records as JSON lines when
`--json` is provided. The exit status SHALL be 0 on success, 1 on a runtime or database error,
2 on a usage error, and 3 when an await finishes without a terminal acknowledgement.

#### Scenario: JSON output
- **WHEN** `--json` is provided to read or follow
- **THEN** each record is emitted as one JSON object per line

#### Scenario: Usage error
- **WHEN** an unknown flag is provided
- **THEN** the CLI exits with status 2

#### Scenario: Await timeout
- **WHEN** an await finishes without a terminal acknowledgement
- **THEN** the CLI exits with status 3
