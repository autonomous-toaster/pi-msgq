# msgq-companions Specification

## Purpose

The `companion` command starts and stops companion agents: a real `pi` process in its own herdr
tab, wired to the message bus so the two ends can talk to each other.

## Requirements

### Requirement: Spawning a companion
`companion spawn <name>` SHALL create a new herdr tab in the caller's workspace and start a real
`pi` process in it, running with the message-bus extension enabled. It SHALL block until the
companion is reachable on the bus, and SHALL then report the granted name, its bus address, its
working directory, and its herdr tab and pane.

#### Scenario: Spawns a reachable companion
- **WHEN** a companion is spawned
- **THEN** it is running in a new herdr tab and is reachable on the bus

#### Scenario: Blocks until reachable
- **WHEN** a companion is spawned
- **THEN** the command returns only after the companion has registered on the bus

#### Scenario: Reports how to reach it
- **WHEN** a spawn succeeds
- **THEN** it reports the granted name, the bus address, the working directory, and the tab and pane

#### Scenario: Not reachable
- **WHEN** the companion does not become reachable within the spawn timeout
- **THEN** the command fails and names the tab and pane it created
### Requirement: Companion naming
The command SHALL grant a unique name. When the requested name is already held by a live companion
it SHALL derive a suffixed alternative, and the bus address SHALL follow the granted name. It SHALL
refuse rather than reuse a taken name, and SHALL stop after a bounded number of attempts.

#### Scenario: Taken name is suffixed
- **WHEN** a companion is spawned with a name a live companion already uses
- **THEN** a suffixed name is granted and reported

#### Scenario: Address follows the granted name
- **WHEN** a suffixed name is granted
- **THEN** the reported bus address matches that granted name

#### Scenario: Bounded attempts
- **WHEN** no alternative name is free within the bound
- **THEN** the command fails without spawning
### Requirement: Companion working directory
The command SHALL accept a working directory, defaulting to the caller's. The companion SHALL run in
that directory so it discovers that project's resources, and the directory SHALL be reported.

#### Scenario: Defaults to the caller's directory
- **WHEN** no working directory is given
- **THEN** the companion runs in the caller's working directory

#### Scenario: Explicit directory
- **WHEN** a working directory is given
- **THEN** the companion runs there, and the reported directory is that one
### Requirement: Wired to its parent
The command SHALL give the companion its parent's bus address, both as an environment variable and
as a command-line argument, so the companion can address its parent and so anything it starts
inherits that context.

#### Scenario: Both channels carry it
- **WHEN** a companion is spawned with a parent address
- **THEN** the environment variable and the command-line argument both carry it

#### Scenario: Inherited by descendants
- **WHEN** the companion starts a process of its own
- **THEN** that process inherits the parent address
### Requirement: Project trust
The command SHALL be able to grant or withhold project trust for the companion explicitly, and
SHALL report a companion that is blocked at startup instead of appearing to hang.

#### Scenario: Trust granted
- **WHEN** project trust is requested
- **THEN** the companion loads the project's protected resources

#### Scenario: Trust withheld
- **WHEN** project trust is refused
- **THEN** the companion skips the project's protected resources

#### Scenario: Blocked at startup
- **WHEN** the companion is blocked at startup
- **THEN** the command reports that rather than waiting for the full timeout
### Requirement: Companion depth
A spawned companion SHALL be told its delegation depth, and the command SHALL refuse to spawn
beyond a cap unless the caller explicitly overrides it.

#### Scenario: Depth is propagated
- **WHEN** a companion is spawned
- **THEN** it is told a depth greater than its parent's

#### Scenario: Cap enforced
- **WHEN** a companion at the cap tries to spawn another
- **THEN** the command refuses unless the override is given
### Requirement: Closing a companion
`companion close <name>` SHALL close that companion's herdr tab, and SHALL report when no such
companion exists.

#### Scenario: Closes the tab
- **WHEN** a live companion is closed
- **THEN** its tab is closed and it is no longer reachable

#### Scenario: Unknown companion
- **WHEN** no live companion has that name
- **THEN** the command reports that without failing destructively
### Requirement: Dry run
The command SHALL be able to print the operations it would perform without performing them, so its
behaviour is inspectable and testable without a live herdr.

#### Scenario: Nothing is executed
- **WHEN** a dry run is requested
- **THEN** the commands that would be run are printed and nothing is created

#### Scenario: Inspectable without herdr
- **WHEN** a dry run is requested where herdr is unavailable
- **THEN** it still prints the commands
### Requirement: Listing companions
`companion list` SHALL report the companions that are running: every named agent herdr reports
that is also a participant on the bus, matched by the name shared between the herdr agent, the pi
session, and the bus address. Each entry SHALL carry the name, the bus address, the herdr status,
and the working directory. The command SHALL report where herdr is unavailable rather than failing,
and SHALL be able to emit its result as one JSON object.

#### Scenario: Lists what is running
- **WHEN** companions are running
- **THEN** each is listed with its name, bus address, herdr status, and working directory

#### Scenario: Richer than the bus roster
- **WHEN** a companion is listed
- **THEN** its herdr status and working directory are reported, which presence alone does not carry

#### Scenario: Not a herdr agent
- **WHEN** a bus participant has no herdr agent of that name
- **THEN** it is reported as a peer but not as a companion

#### Scenario: Machine readable
- **WHEN** JSON output is requested
- **THEN** the result is one JSON object whose entries carry the same fields

#### Scenario: Outside herdr
- **WHEN** herdr is unavailable
- **THEN** the command reports that rather than failing destructively
