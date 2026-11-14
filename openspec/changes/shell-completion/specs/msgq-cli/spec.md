# Spec Delta

## ADDED Requirements

### Requirement: A listing has a names form

T2.1 ALWAYS, `msgq topics --names` SHALL print one address per line.
T2.1 ALWAYS, `msgq sessions --names` SHALL print one participant address per line.
T2.1 ALWAYS, `msgq consumers --names` SHALL print one consumer name per line.
T2.1 ALWAYS, a names form SHALL print no count, no header, no offset and no JSON.

#### Scenario: Names are addressable
- **WHEN** a caller runs `msgq topics --names` on a bus that holds two addresses
- **THEN** the output SHALL hold exactly the two addresses, one per line

#### Scenario: Names carry no bookkeeping
- **WHEN** a caller runs a names form
- **THEN** the output SHALL hold no column separator and no header line


#### Scenario: Names respect the caller's namespace
- **WHEN** a caller runs `msgq consumers --names` on a bus that holds a cursor of another principal
- **THEN** the output SHALL name no consumer of that principal

### Requirement: Two output forms are refused together

T2.2 IF a caller asks for a names form and JSON together, THEN the CLI SHALL exit with a usage error.

#### Scenario: Two output forms are refused together
- **WHEN** a caller requests a names form and JSON
- **THEN** the CLI SHALL exit with a usage error

### Requirement: A closed value is accepted from the members of a closed set

T1.1 ALWAYS, the CLI SHALL accept a value of a closed set from the members of that closed set.
T1.2 ALWAYS, the CLI SHALL validate an acknowledgement phase set where the argument is parsed.
T1.3 ALWAYS, the help text SHALL name the members of a closed set.

#### Scenario: The help names the members
- **WHEN** a caller reads the help of a command with a closed-set argument
- **THEN** the help SHALL name every member of that closed set

### Requirement: A value outside a closed set is refused

T1.1 IF a supplied value is not a member of the closed set of the argument, THEN the CLI SHALL exit with a usage error.

#### Scenario: An unknown record type
- **WHEN** a caller publishes with `--type` naming a value outside the members
- **THEN** the CLI SHALL exit with a usage error and SHALL store no record

#### Scenario: An unknown acknowledgement phase
- **WHEN** a caller publishes with `--ack` naming a phase outside the protocol phases
- **THEN** the CLI SHALL exit with a usage error and SHALL store no record

### Requirement: The completion verb describes one shell

T3.1 ALWAYS, `msgq completions <shell>` SHALL accept `zsh` alone.

#### Scenario: The supported shell is named in the help
- **WHEN** a caller reads the help of `msgq completions`
- **THEN** the help SHALL name `zsh`

### Requirement: An unknown shell is refused

T4.2 IF the caller names another shell, THEN the CLI SHALL exit with a usage error.
T4.2 IF the caller names no shell, THEN the CLI SHALL exit with a usage error.

#### Scenario: Another shell is refused
- **WHEN** a caller asks for a bash completion
- **THEN** the CLI SHALL exit with a usage error and SHALL print no script
