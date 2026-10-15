## ADDED Requirements

### Requirement: Record identity
Every record SHALL have an id of the form `<address>#<offset>`: the canonical address whose stream
holds it, and the offset it was assigned. The id SHALL be derived from what the
record already carries and SHALL NOT be stored in the record, so it applies to every record ever
published. A record's reply reference SHALL be an offset in the stream named by that record's own
`from`, so the record a reference names is always `<from>#<reply_to>`.

#### Scenario: Derived, not stored
- **WHEN** a record is published
- **THEN** its id is the canonical address it was published to and the offset assigned there, and the stored record carries no id field

#### Scenario: A reference identifies its record
- **WHEN** a published record carries `reply_to`
- **THEN** the record it refers to is the one at that offset in the stream named by that record's `from`

#### Scenario: A reference is exact rather than inferred
- **WHEN** a reply is published to the address of the session that sent the record it answers
- **THEN** its `reply_to` is the offset of that record in the answering session's stream, which is the stream named by the reply's `from`

#### Scenario: Earlier records have ids too
- **WHEN** a record published before this change is read
- **THEN** an id can be formed for it from its address and its offset

## MODIFIED Requirements

### Requirement: Publish
`msgq publish <address> [message]` SHALL append one message to the stream named by the canonical address and print the assigned offset to stdout. It SHALL be able to report the record's id instead of that offset, to request acknowledgements on the record it publishes, and to reference the message it is replying to, by offset or by id.

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

#### Scenario: Reporting the id
- **WHEN** the id is asked for instead of the bare offset
- **THEN** the printed value is the canonical address and the assigned offset joined by `#`

#### Scenario: Referencing by id
- **WHEN** `--reply-to` names an id whose address is the record's own sender
- **THEN** the published record carries that id's offset as its reply reference

#### Scenario: Referencing another sender's id
- **WHEN** `--reply-to` names an id whose address is not the record's own sender
- **THEN** the command exits with a usage error and writes nothing
