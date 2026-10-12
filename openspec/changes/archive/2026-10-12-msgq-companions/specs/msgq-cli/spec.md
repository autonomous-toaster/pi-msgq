## ADDED Requirements

### Requirement: Awaiting a reply
`msgq ask <address> [message]` SHALL publish a message that asks for an answer and then wait for
one. It SHALL finish when the recipient replies, when a terminal acknowledgement for that message
arrives, or when its wait window closes. A reply SHALL be printed as the command's output; the
other two outcomes SHALL be reported as such. None of the three SHALL be an error. The wait window
SHALL default to a finite value and MAY be disabled.

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

## MODIFIED Requirements

### Requirement: Publish
`msgq publish <address> [message]` SHALL append one message to the stream named by the canonical address and print the assigned offset to stdout. It SHALL be able to request acknowledgements on the record it publishes, and to reference the message it is replying to.

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

#### Scenario: Replying to a message
- **WHEN** `--reply-to` is given
- **THEN** the published record carries that offset as its reply reference
