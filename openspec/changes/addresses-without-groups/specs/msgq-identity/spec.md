# Spec Delta

## MODIFIED Requirements

### Requirement: Identity is a principal

T1.1 ALWAYS, one identity row SHALL name one principal.
T1.1 ALWAYS, the name of an identity row SHALL hold a principal name rather than an address.
T1.1 ALWAYS, the scopes of an identity row SHALL hold the addresses that the principal may reach.
T1.1 ALWAYS, a scope SHALL cover exactly the address it names.

#### Scenario: One row per user
- **WHEN** an operator registers a principal
- **THEN** the bus SHALL store one identity row for that principal

#### Scenario: A scope covers a subtree
- **WHEN** a scope names an address
- **THEN** the scope SHALL cover that address and SHALL NOT cover a name that merely shares its prefix

#### Scenario: A scope covers one stream
- **WHEN** a scope names `session/alice-1`
- **THEN** it SHALL cover that stream and SHALL NOT cover `session/alice-2`

#### Scenario: No scheme names a set
- **WHEN** an operator writes a scope
- **THEN** the scope SHALL name one party's address, because no scheme addresses a set of them any more
