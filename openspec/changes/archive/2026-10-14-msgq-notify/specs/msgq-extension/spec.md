## ADDED Requirements

### Requirement: Reference annotation
When a delivered record carries a reply reference, the delivered content SHALL identify the record
it refers to by its id, and SHALL quote that record's sender and body when the record can be read.
The lookup SHALL be resolved at delivery and SHALL NOT be remembered between records, and it SHALL
NOT advance any consumer cursor. When the referenced record cannot be read, the annotation SHALL
still name its id rather than failing or omitting it.

#### Scenario: The answer says what it answers
- **WHEN** a record arrives carrying `reply_to` and the referenced record can be read
- **THEN** the delivered content names the referenced id and quotes the record, so the session knows what it is being told about

#### Scenario: Acknowledgements are annotated the same way
- **WHEN** an acknowledgement arrives carrying `reply_to`
- **THEN** the delivered content names the record being acknowledged, so progress and answers are read the same way

#### Scenario: Nothing is remembered
- **WHEN** a record arrives referencing something this session sent before the session started, or from a shell
- **THEN** the reference is still resolved, because nothing depends on what this session recorded earlier

#### Scenario: The reference cannot be read
- **WHEN** the referenced record is no longer in the stream
- **THEN** the delivered content names the id and delivery continues

#### Scenario: Reading is not consuming
- **WHEN** a reference is resolved
- **THEN** no consumer cursor moves, and no record is acknowledged or redelivered as a result

### Requirement: Shipped skill
The extension SHALL make a skill available to every session that loads it, so the model is told how
a process outside the session reaches this one. The skill SHALL be advertised by name and
description without loading its instructions, and loading the extension SHALL NOT depend on the
skill being present or valid.

#### Scenario: The skill is advertised
- **WHEN** a session loads the extension and lists its available skills
- **THEN** the notify skill is offered by name and description

#### Scenario: The instructions load on demand
- **WHEN** the extension is loaded
- **THEN** only the skill's name and description are added, and its instructions are read when a task calls for them

#### Scenario: A broken skill does not break the extension
- **WHEN** the skill directory is missing or malformed
- **THEN** the extension still loads, consumes, and delivers
