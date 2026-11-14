# msgq-completion Specification

## Purpose
What a tab offers at the prompt, where each offered value comes from, what a tab is allowed to cost,
and how the offered script stays a description of the CLI that ships it.

## Requirements

### Requirement: The script is generated from the command definition

T3.1 ALWAYS, `msgq completions zsh` SHALL emit a completion script on standard output.
T3.1 ALWAYS, the emitted script SHALL be built from the command definition the CLI runs.
T3.1 ALWAYS, the emitted script SHALL name the binary `msgq`.

#### Scenario: The definition is the only source
- **WHEN** an operator adds a flag to the command definition and regenerates the script
- **THEN** the emitted script SHALL offer that flag

#### Scenario: Emission changes nothing else
- **WHEN** a caller runs `msgq completions zsh`
- **THEN** the CLI SHALL exit zero and SHALL store no record

### Requirement: A closed value completes its members

T3.2 ALWAYS, a value the CLI accepts from a closed set SHALL complete to the members of that set.
T3.2 ALWAYS, the completion SHALL offer no member outside that set.

#### Scenario: The record type
- **WHEN** a caller completes `--type`
- **THEN** the completion SHALL offer `prompt`, `steer`, `follow_up`, `note`, `command` and `event`

#### Scenario: The path detail
- **WHEN** a caller completes `card --paths`
- **THEN** the completion SHALL offer `relative`, `full` and `none`

### Requirement: An open value is suggested but never restricted

T3.2 ALWAYS, the completion SHALL suggest a common form for a value the CLI accepts as a pattern.
T3.2 ALWAYS, the completion SHALL accept a value outside the suggested forms.

#### Scenario: An uncommon phase set is accepted
- **WHEN** a caller types an acknowledgement phase set outside the suggested forms
- **THEN** the completion SHALL accept the typed value

#### Scenario: An uncommon age is accepted
- **WHEN** a caller types an age outside the suggested forms for `prune --older-than`
- **THEN** the completion SHALL accept the typed value

### Requirement: An address completes from the live bus

T3.3 ALWAYS, the completion SHALL offer the addresses that hold a record.
T3.3 ALWAYS, the completion SHALL offer a live participant address beside a written address.
T3.3 ALWAYS, the completion SHALL obtain the offered addresses with one enumeration per tab.

#### Scenario: A written address
- **WHEN** a caller completes the address of `read`
- **THEN** the completion SHALL offer each address that holds a record

#### Scenario: A live participant
- **WHEN** a participant holds an unexpired presence record
- **THEN** the completion SHALL offer the address of that participant

### Requirement: A consumer name completes from the caller's cursors

T3.3 ALWAYS, the completion SHALL offer the consumer names with a cursor owned by the caller.
T3.3 ALWAYS, the completion SHALL offer a topic that holds a cursor of the caller.

#### Scenario: A consumer of one cursor
- **WHEN** a caller completes the consumer of `cursor get`
- **THEN** the completion SHALL offer each consumer name with a cursor owned by the caller

#### Scenario: A topic of one cursor
- **WHEN** a caller completes the topic of `cursor get`
- **THEN** the completion SHALL offer each topic that holds a cursor owned by the caller

### Requirement: A completion offers only what the caller may read

T3.3 ALWAYS, a completion SHALL name no address outside the scopes of the caller.
T3.3 ALWAYS, a completion SHALL name no consumer owned by another principal.

#### Scenario: Another tenant's address is absent
- **WHEN** a caller completes an address on a bus that holds an address of another principal
- **THEN** the completion SHALL offer no address of that principal

#### Scenario: Another tenant's consumer is absent
- **WHEN** a caller completes a consumer name on a bus that holds a cursor of another principal
- **THEN** the completion SHALL offer no consumer of that principal

### Requirement: A tab writes no record

T3.4 ALWAYS, a completion SHALL store no record.
T3.4 ALWAYS, a completion SHALL advance no cursor.

#### Scenario: A tab on an empty bus
- **WHEN** a caller completes an address on a bus that holds no record
- **THEN** the bus SHALL hold no record and no cursor afterwards

### Requirement: A test compares the script against the command definition

T3.5 ALWAYS, a test SHALL compare the emitted script against the command definition.

#### Scenario: An unemitted flag fails the suite
- **WHEN** a flag is added to the definition and the emitter is not updated
- **THEN** the completion test SHALL fail

### Requirement: An unemitted item fails the test

T3.5 IF the definition holds a subcommand the script omits, THEN the test SHALL fail.
T3.5 IF the definition holds a long flag the script omits, THEN the test SHALL fail.
T3.5 IF the definition holds a member of a closed set, THEN the test SHALL check every member.

#### Scenario: An unemitted member fails the suite
- **WHEN** a member is added to a closed set and the emitted script is not updated
- **THEN** the completion test SHALL fail

### Requirement: The script reaches the shell's function path

T4.1 ALWAYS, an operator SHALL install the emitted script as `_msgq` in a directory on `fpath`.
T4.1 ALWAYS, the repository SHALL provide one command that writes the script to that directory.
T4.3 ALWAYS, the documentation SHALL state the supported shell.
T4.3 ALWAYS, the documentation SHALL state the bus a completion opens when the caller configures nothing.
T4.3 ALWAYS, the documentation SHALL state that a directory-derived address is not completed.

#### Scenario: One command installs it
- **WHEN** a developer runs the install command
- **THEN** the shell's function path SHALL hold `_msgq`

#### Scenario: A tab reaches the bus the caller configured
- **WHEN** a caller completes an address with `MSGQ_DB` set
- **THEN** the completion SHALL enumerate the addresses of that bus

#### Scenario: The limits are written down
- **WHEN** a reader consults the install instructions
- **THEN** the instructions SHALL state the supported shell and what a tab does not offer
