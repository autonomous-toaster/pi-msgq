## MODIFIED Requirements

### Requirement: Awaiting a reply
`msgq ask <address> [message]` SHALL publish a message that asks for an answer and then wait for
one. It SHALL finish when the recipient replies, when a terminal acknowledgement for that message
arrives, or when its wait window closes. A reply SHALL be printed as the command's output; the
other two outcomes SHALL be reported as such. None of the three SHALL be an error. The wait window
SHALL default to a finite value and MAY be disabled.

With `--no-wait` the command SHALL publish the same record and return as soon as it is published,
printing its offset, without waiting for any reply. The published record SHALL be identical to the
one a blocking ask publishes, so a client cannot tell the two apart on the wire.

#### Scenario: Reply arrives
- **WHEN** the recipient replies to the message
- **THEN** the reply is printed and the command succeeds

#### Scenario: Finished without replying
- **WHEN** a terminal acknowledgement for the message arrives and no reply does
- **THEN** the command reports that the recipient finished without replying and succeeds

#### Scenario: Still running
- **WHEN** the wait window closes with neither a reply nor a terminal acknowledgement
- **THEN** the command reports that the recipient is still running and succeeds

#### Scenario: Wait window disabled
- **WHEN** the wait window is disabled
- **THEN** the command waits until the recipient replies or finishes

#### Scenario: Asking without waiting
- **WHEN** `--no-wait` is provided
- **THEN** the command prints the offset of the published question and returns before any reply exists

#### Scenario: One question, one shape
- **WHEN** the same question is asked with and without `--no-wait`
- **THEN** the two records differ only in the offset they are stored at
