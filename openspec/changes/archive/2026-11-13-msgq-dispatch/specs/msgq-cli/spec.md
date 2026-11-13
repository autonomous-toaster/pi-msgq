## ADDED Requirements

### Requirement: Command records
A record whose `type` is `command` SHALL carry a `message` that is a command line: the name of a
command the receiving session may run, followed by any arguments for it. The command SHALL be the
whole body, because an invocation cannot be wrapped, so anything the sender wants known about
itself SHALL travel outside the body rather than inside it. In every other respect a command record
is an ordinary record: the same envelope, the same version, the same addressing, and the same
options.

#### Scenario: The command is the whole body
- **WHEN** a command record is published
- **THEN** its `message` is exactly the command line, with nothing prefixed to it

#### Scenario: An older consumer
- **WHEN** a consumer that does not know the `command` type reads such a record
- **THEN** it displays it without error, as it would for any unknown type

#### Scenario: Nothing on the bus validates the command
- **WHEN** a command record names something no session can run
- **THEN** the record is published and accepted all the same, since the bus does not know which commands exist

## MODIFIED Requirements

### Requirement: Message record contract
A published message SHALL be a JSON object carrying `v`, `type`, `from`, and `to`, and for message-bearing types a `message`. `v` MUST be `1`. `type` SHALL be one of `prompt`, `steer`, `follow_up`, `note`, `command`, or `event`, and consumers MUST tolerate unknown types. `images`, `reply_to`, and `meta` SHALL be optional. When `type` is `event`, an `event` object MUST be present in place of `message`. A record SHALL NOT carry a publisher-supplied timestamp field.

#### Scenario: Shorthand record
- **WHEN** a message is published in shorthand text mode
- **THEN** the stored record contains `v`, `type`, `from`, `to`, and `message`

#### Scenario: Unknown type tolerated
- **WHEN** a consumer reads a record whose `type` it does not recognize
- **THEN** it displays the record without error

#### Scenario: Event record
- **WHEN** `type` is `event`
- **THEN** the record carries an `event` object and is accepted by the bus

#### Scenario: Command record
- **WHEN** `type` is `command`
- **THEN** the record carries a `message` naming a command, and is accepted by the bus
