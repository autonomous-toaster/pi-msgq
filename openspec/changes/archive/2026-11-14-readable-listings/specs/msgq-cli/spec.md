# Spec Delta

## ADDED Requirements

### Requirement: A listing has a card form

T2.3 ALWAYS, `msgq topics --card` SHALL print two tab-separated columns: the address, then a compact description.
T2.3 ALWAYS, `msgq sessions --card` SHALL print the address, then the compact description of the card.
T2.3 ALWAYS, the compact description SHALL name the project and the branch, separated by a slash.
T2.3 ALWAYS, the compact description SHALL name the last component of the path.
T2.3 ALWAYS, the compact description SHALL contain no separator inside either column.

#### Scenario: An address with a card
- **WHEN** a participant advertises an address with a card naming project, branch and path
- **THEN** `--card` SHALL print the address, a tab, and the project and branch with the last path component

#### Scenario: Two columns are cut cleanly
- **WHEN** a caller cuts the first column of a card form
- **THEN** the output SHALL hold one address per line, whatever the second column holds

### Requirement: A card description is bounded

T2.5 ALWAYS, the compact description SHALL fit a bounded width whatever the length of a name.
T2.5 ALWAYS, a shortened name SHALL keep both ends and lose the middle.
T2.5 ALWAYS, the address SHALL stand first in the row. T4.1, T4.2

#### Scenario: A long repository name
- **WHEN** a card names a project and a branch longer than the budget
- **THEN** the description SHALL be shortened in the middle and SHALL stay within the budget

#### Scenario: A narrow terminal keeps the address
- **WHEN** a picker shows the card form in a terminal narrower than the row
- **THEN** the truncated part SHALL be the description, not the address

### Requirement: A card form falls back rather than varying

T2.4 IF an address has no live participant, THEN the card form SHALL print `-` in the second column.
T2.4 IF a card names neither project, branch nor path, THEN the card form SHALL print `-` in the second column. T2.1, T2.2, T2.3
T2.4 IF a caller asks for two output forms together, THEN the CLI SHALL exit with a usage error.

#### Scenario: A topic no participant advertises
- **WHEN** a caller runs the card form on a bus where no participant advertises one of the addresses
- **THEN** that address SHALL be printed with `-` as its description

#### Scenario: Three forms are refused in pairs
- **WHEN** a caller asks for the card form together with `--names` or `--json`
- **THEN** the CLI SHALL exit with a usage error and SHALL print no listing
