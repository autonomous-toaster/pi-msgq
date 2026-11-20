# Spec Delta

## MODIFIED Requirements

### Requirement: Address grammar and canonicalization
An address SHALL identify both a recipient and the stream it is written to. When an address contains `/`, its prefix MUST be one of `session`, `cli`, or `bus`; any other prefix is a usage error. An address without `/` SHALL be canonicalized to `session/<name>`. Addresses SHALL be case-sensitive with surrounding whitespace trimmed. An empty address, or a scheme-only address such as `session/`, SHALL be rejected.

#### Scenario: Bare name canonicalization
- **WHEN** a message is published to `reviewer`
- **THEN** the stream `session/reviewer` is written and `topics` lists `session/reviewer`

#### Scenario: Unknown prefix rejected
- **WHEN** an address such as `foo/bar` is used
- **THEN** the CLI exits with a usage error and writes nothing

#### Scenario: A removed scheme is rejected
- **WHEN** an address such as `group/backend` or `cwd/Users-me-proj` is used
- **THEN** the CLI exits with a usage error, naming the schemes that exist

#### Scenario: Scheme-only address rejected
- **WHEN** an address such as `session/` is used
- **THEN** the CLI exits with a usage error

#### Scenario: Case sensitivity
- **WHEN** `session/Api` and `session/api` are used
- **THEN** they address two distinct streams
