## Purpose

The contract a client other than pi may depend on: one envelope, one tolerance rule, one versioning
policy, and the conventions built from records that already exist — so that a shell script, another
agent framework, or a future harness can speak the bus without reading pi's source.

## ADDED Requirements

### Requirement: The envelope a client may depend on

A record SHALL be a JSON object carrying `v`, `type`, `from` and `to`, and MAY carry `message`,
`event`, `reply_to` and `meta`. `reply_to` SHALL name the offset of the record being answered. `meta`
SHALL be an open object: clients SHALL preserve keys they do not recognize and SHALL NOT fail on
them. No other top-level field SHALL be required to read or deliver a record.

#### Scenario: Minimal record
- **WHEN** a client publishes with only a sender, an address and a message
- **THEN** the record carries `v`, `type`, `from`, `to` and `message`, and readers accept it

#### Scenario: Unknown meta keys
- **WHEN** a record carries a `meta` key a reader does not recognize
- **THEN** the reader ignores it, preserves it where it rewrites records, and does not fail

#### Scenario: Answering by reference
- **WHEN** a record answers another
- **THEN** it carries that record's offset as `reply_to`, which is enough to correlate the two

### Requirement: Vocabulary tolerance

`type` SHALL be a short string, and the known set SHALL be a convention rather than a closed list.
A reader SHALL deliver a record whose `type` it does not recognize as ordinary content. A publisher
SHALL NOT be required to know what the receiver does with a type.

#### Scenario: Unknown type reaches the reader
- **WHEN** a client publishes a `type` that the receiving implementation does not know
- **THEN** the record is delivered as content and consumption continues

#### Scenario: Known types are documented, not enforced
- **WHEN** the set of known types is extended
- **THEN** no reader that predates the extension stops reading

### Requirement: Version tolerance

`v` SHALL identify the envelope version and SHALL change only for a change that a reader of the
previous version cannot survive. Additive changes — a new `meta` key, a new event kind, a new type,
a new address convention — SHALL keep `v` unchanged. A reader SHALL reject an unknown `v` with a
reason that names the version it found, rather than misreading the record.

#### Scenario: Additive change needs no bump
- **WHEN** a convention is added on top of the existing envelope
- **THEN** `v` is unchanged, and readers of the previous version keep working

#### Scenario: Rejected future version
- **WHEN** a record carries a `v` the reader does not implement
- **THEN** the record is skipped with a reason naming that version, and consumption continues

### Requirement: One vocabulary, checked against fixtures

The record vocabulary SHALL be defined in one place, and every shipped implementation SHALL be
tested against the same fixtures. A fixture that one implementation accepts SHALL be accepted by
every other, and a fixture that one rejects SHALL be rejected by every other.

#### Scenario: Fixtures agree
- **WHEN** the conformance fixtures are run against two implementations
- **THEN** both accept and reject the same records

#### Scenario: Drift is caught
- **WHEN** an implementation's known-type list is edited and the other's is not
- **THEN** a conformance test fails

### Requirement: Acknowledgements are a convention over event records

An acknowledgement SHALL be an `event` record whose `event.kind` is `ack`, published from the
acknowledging address to the address that asked. The request SHALL be expressed in the request's own
`meta.ack`, and the phases SHALL be `received`, `started` and `completed`, in that order. Nothing
about acknowledgements SHALL exist in the envelope.

#### Scenario: A client asks and another answers
- **WHEN** a publisher requests acknowledgements and the receiver is not pi
- **THEN** the receiver can satisfy the request by publishing `ack` event records, using no
  implementation of its own beyond the envelope

#### Scenario: An acknowledgement is not a message
- **WHEN** an acknowledgement arrives
- **THEN** a reader treats it as progress about another record, not as a new message to answer

### Requirement: Presence is a convention over event records

Presence SHALL be an `event` record with `event.kind` of `presence`, carrying a `state` and a
lifetime, published to one well-known address that every participant and the roster agree on. A
participant SHALL be considered live while its most recent presence record is younger than its
lifetime, as judged by the bus's own recorded time rather than the publisher's clock. A participant
that shuts down cleanly SHALL publish a presence record that retracts it.

#### Scenario: Any client can appear
- **WHEN** a process with nothing but the command-line client publishes a presence record
- **THEN** it appears in the roster alongside pi sessions

#### Scenario: Retraction is immediate
- **WHEN** a participant retracts its presence on clean shutdown
- **THEN** it leaves the roster without waiting for the lifetime to elapse

#### Scenario: Crash ages out
- **WHEN** a participant stops without retracting
- **THEN** it leaves the roster once its lifetime has elapsed

### Requirement: Correlation travels in `meta`

A client that starts work on behalf of an existing record SHALL be able to carry a correlation
identifier in `meta.trace`, and SHALL be able to reconstruct a tree of related records from it. This
SHALL NOT require a new envelope field, and a client that does not use it SHALL be unaffected.

#### Scenario: A delegation is reconstructable
- **WHEN** a sender stamps an identifier on the request and every participant stamps the same
  identifier on the records it sends in that work
- **THEN** the records of that work can be listed together, across addresses, by that identifier

#### Scenario: Ignoring it is free
- **WHEN** a client neither reads nor writes `meta.trace`
- **THEN** every other behaviour is unchanged
