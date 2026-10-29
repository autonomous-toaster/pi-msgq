## ADDED Requirements

### Requirement: Launcher contract

Starting a companion SHALL be understood as one implementation of a stated contract, not as a
capability of the bus: a launcher SHALL start a `pi` process that loads the extension for a stated
directory, SHALL name it so that it is reachable at an address, SHALL tell it whom to report to and
whom to trust, SHALL pass the bus location and the tool paths it needs, SHALL wait until that address
appears on the roster, and SHALL be able to stop it. `companion` SHALL satisfy this contract and SHALL
be documented as one implementation of it. Every requirement of this capability SHALL be achievable
by any other launcher that satisfies the contract.

#### Scenario: Another launcher
- **WHEN** a launcher other than `companion` follows the contract
- **THEN** the companions it starts are indistinguishable on the bus from those `companion` starts

#### Scenario: The bus without a launcher
- **WHEN** no launcher is installed
- **THEN** publishing, reading, following, the roster and both agent tools still work, and only
  starting a companion is unavailable

#### Scenario: The contract is readable without the dependency
- **WHEN** a reader has no herdr and no launcher installed
- **THEN** the dry run of a spawn prints the exact commands a launcher would run, so the contract can be
  read without satisfying it

#### Scenario: The prerequisite stays with the launcher
- **WHEN** the bus's requirements are documented
- **THEN** the launcher's own prerequisite is documented with the launcher, and is not required to
  publish, read, follow, or appear on the roster
