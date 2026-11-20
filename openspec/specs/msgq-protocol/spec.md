# msgq-protocol Specification

## Purpose

The contract a client other than pi may depend on: one envelope, one tolerance rule, one versioning
policy, and the conventions built from records that already exist — so that a shell script, another
agent framework, or a future harness can speak the bus without reading pi's source.

## Requirements

### Requirement: The envelope a client may depend on

T1.1 ALWAYS, a record SHALL be a JSON object.
T1.1 ALWAYS, a record SHALL carry `v`, `type`, `from`, and `to`.
T1.1 ALWAYS, a record MAY carry `message`, `event`, `reply_to`, and `meta`.
T1.1 ALWAYS, a record id SHALL name an address together with an offset.
T1.1 ALWAYS, offsets SHALL be unique within a stream.
T1.1 ALWAYS, a client SHALL NOT assume offset uniqueness across the bus.
T1.1 ALWAYS, `reply_to` SHALL name an offset in the stream of the answering record's `from`.
T1.1 ALWAYS, `meta` SHALL be an open object.
T1.1 ALWAYS, a client SHALL preserve an unrecognized `meta` key.
T1.1 ALWAYS, a client SHALL NOT fail on an unrecognized `meta` key.
T1.1 ALWAYS, no other top-level field SHALL be required.

#### Scenario: Minimal record
- **WHEN** a client publishes with only a sender, an address and a message
- **THEN** the record SHALL carry `v`, `type`, `from`, `to` and `message`

#### Scenario: Unknown meta keys
- **WHEN** a record carries a `meta` key that a reader does not recognize
- **THEN** the reader SHALL ignore the key and SHALL preserve the key

#### Scenario: Answering by reference
- **WHEN** a record answers another record
- **THEN** the answering record SHALL carry the answered offset as `reply_to`

#### Scenario: An offset is scoped to its stream
- **WHEN** two streams each hold a record at the same offset
- **THEN** the bus SHALL name the two records by two different addresses

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

T1.4 ALWAYS, one place SHALL define the record vocabulary.
T1.4 ALWAYS, every implementation SHALL pass the same fixtures.
T1.4 ALWAYS, every implementation SHALL accept the same fixtures.
T1.4 ALWAYS, every implementation SHALL reject the same fixtures.
T7.1 ALWAYS, the fixtures SHALL cover bus behaviour.

#### Scenario: Fixtures agree
- **WHEN** the conformance fixtures run against two implementations
- **THEN** both implementations SHALL accept and reject the same records

#### Scenario: Drift is caught
- **WHEN** an editor edits one implementation's known-type list and leaves the other list alone
- **THEN** a conformance test SHALL fail

#### Scenario: Engines agree on behaviour
- **WHEN** the behavioural fixtures run against each storage engine
- **THEN** both engines SHALL produce the same outcome for every fixture

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

T5.5 ALWAYS, presence SHALL be an `event` record with `event.kind` of `presence`.
T5.5 ALWAYS, a presence record SHALL carry a `state` and a lifetime.
T5.5 ALWAYS, presence SHALL travel to one well-known address per scope, under the `bus` scheme.
T5.5 ALWAYS, a presence record SHALL NOT be sealed: it is public metadata that every participant reads.
T5.5 ALWAYS, a participant SHALL count as live while the newest presence record is younger than the declared lifetime.
T5.5 ALWAYS, the bus SHALL judge liveness by recorded time.
T5.5 ALWAYS, a roster SHALL list only the participants of the roster scope.
T5.5 ALWAYS, a clean shutdown SHALL publish a presence record that retracts the participant.

#### Scenario: Any client can appear
- **WHEN** a process publishes a presence record with nothing but the command-line client
- **THEN** the roster SHALL list the process alongside pi sessions

#### Scenario: Retraction is immediate
- **WHEN** a participant retracts presence on clean shutdown
- **THEN** the roster SHALL drop the participant without waiting for the lifetime

#### Scenario: Crash ages out
- **WHEN** a participant stops without retracting presence
- **THEN** the roster SHALL drop the participant after the declared lifetime elapses

#### Scenario: Scopes do not see each other
- **WHEN** two participants publish presence in different scopes
- **THEN** each roster SHALL list only the participants of one scope

#### Scenario: The roster is readable by everybody
- **WHEN** a client with no key of its own reads the presence address
- **THEN** the record SHALL be readable and SHALL NOT be sealed, and its signature SHALL carry the
  participant's agreement key

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

### Requirement: Presence may carry a locator card

A presence record MAY carry an optional `event.card` object that locates the participant: the project
and, where available, the git branch, worktree, and path it is working in. The field is additive, so
a `v: 1` reader that does not know it MUST still accept the record. The card SHALL be a flat object
whose reference-client fields are `project`, `branch`, `worktree`, and `path`; it SHALL NOT carry
`state`, current task, progress, or a work summary.

- `project` is the stable discovery key: a normalized `origin` remote in `host/owner/repo` form when
  one exists, otherwise the repository root directory name, or the current directory name outside
  git.
- `branch` is the current git branch when one exists and is omitted when detached.
- `worktree` is the worktree root directory name and is omitted outside git.
- `path` is the current directory relative to the repository root by default, may be absolute when
  the publisher opts in, and is omitted outside git or when path reporting is disabled.

#### Scenario: A locator card is accepted

- **WHEN** a presence record carries `event.card` with locator fields
- **THEN** a `v: 1` reader accepts the record and exposes the card without changing presence state or lifetime

#### Scenario: No card is still valid

- **WHEN** a presence record carries no `card`
- **THEN** the record is accepted exactly as before

#### Scenario: Unknown card keys are tolerated

- **WHEN** a card carries a key the reader does not know
- **THEN** the reader ignores that key for display and does not reject the record

#### Scenario: The card is not a profile

- **WHEN** a card is published
- **THEN** it does not carry current state, task, progress, summary, capabilities, version, provider, or documentation fields

#### Scenario: Path can be omitted

- **WHEN** a publisher disables path reporting or is outside git in relative mode
- **THEN** the card remains valid without `path`

### Requirement: The retention window is declared

T1.2 ALWAYS, the bus SHALL declare a retention window.
T1.2 ALWAYS, the bus SHALL report the retention window.

#### Scenario: The window is reportable
- **WHEN** a client asks the bus about the bus
- **THEN** the client SHALL learn the retention window

### Requirement: Retention removes older records

T1.2 AFTER the retention window elapses, the bus SHALL remove older records.
T1.2 AFTER a record exceeds the retention window, a client SHALL NOT assume persistence of the record.
T1.2 AFTER a consumer falls behind the window, the bus SHALL let the consumer lose records.

#### Scenario: History is a window, not an archive
- **WHEN** a client reads the oldest available records of an address
- **THEN** the client SHALL receive the oldest retained records

#### Scenario: A lagging consumer does not pin history
- **WHEN** a consumer position is older than the retention window
- **THEN** the bus SHALL remove records on schedule

### Requirement: The envelope is always readable

T1.5 ALWAYS, a reader SHALL read `v`, `type`, `from`, `to`, `reply_to`, and `meta` without interpreting a body.
T1.5 ALWAYS, a reader SHALL read presence bodies without interpreting opaque content.
T1.5 ALWAYS, a reader SHALL read acknowledgement bodies without interpreting opaque content.
T1.5 ALWAYS, a reader SHALL report an error for an unreadable body.
T1.5 ALWAYS, a reader SHALL NOT treat an unreadable body as content.

#### Scenario: A body may be unreadable
- **WHEN** a reader encounters a record with a body that the reader cannot interpret
- **THEN** the reader SHALL name the address, the offset, the sender, and the type

#### Scenario: An opaque body is not acted on
- **WHEN** a reader cannot interpret a record body
- **THEN** the reader SHALL report an error

#### Scenario: Acknowledgement and presence stay readable
- **WHEN** a client routes an acknowledgement or builds a roster
- **THEN** the client SHALL need no capability beyond reading the record

### Requirement: A body may declare its encoding

T1.3 ALWAYS, an encoded body SHALL carry a `meta.enc` declaration.
T1.3 ALWAYS, a client SHALL preserve an unrecognized `meta.enc` key.
T1.3 ALWAYS, a declaration SHALL name the algorithm, the additional data it was sealed under, and the parties who can open it.
T6.3 ALWAYS, a reader SHALL treat an unhonourable `meta.enc` as an opaque body.

#### Scenario: Declaration travels in meta
- **WHEN** a sender encodes a body for a reader
- **THEN** `meta.enc` SHALL name the encoding and the parties who can open it

#### Scenario: Ignoring the declaration is harmless
- **WHEN** a client reads a record with a `meta.enc` value that the client does not understand
- **THEN** the client SHALL preserve the key and SHALL NOT fail

#### Scenario: The declaration names who can open it
- **WHEN** a body is sealed to recipients
- **THEN** the declaration SHALL name, for each of them, the fingerprint of the key that opens it, and
  SHALL name no key material beyond the ephemeral public keys

#### Scenario: One binding
- **WHEN** a body is sealed
- **THEN** its additional data SHALL bind the canonical sender and the address, and there SHALL be no
  older binding to accept

### Requirement: Answering is requested in `meta.reply`

T1.3 ALWAYS, a sender that wants an answer SHALL set `meta.reply` to true.
T1.3 ALWAYS, a reader SHALL NOT treat a record without `meta.reply` as a question.
T1.3 ALWAYS, a reader SHALL NOT interpret the body of a reply request.

#### Scenario: A question is marked as one
- **WHEN** a sender publishes a record with `meta.reply` set to true
- **THEN** a reader SHALL detect the request without reading the body

#### Scenario: Ordinary records owe nothing
- **WHEN** a record carries no `meta.reply`
- **THEN** a reader SHALL NOT treat the record as a question

### Requirement: An encoded body is declared

T2.1 ALWAYS, a sealed body SHALL declare an algorithm and the parties who can open it in the record metadata.
T2.1 ALWAYS, a sealed body MAY declare more than one recipient, each with its own ephemeral key.
T2.1 ALWAYS, the record envelope SHALL stay readable.
T2.1 ALWAYS, a presence body SHALL stay readable.
T2.1 ALWAYS, an acknowledgement body SHALL stay readable.

#### Scenario: A reader that knows no encoding
- **WHEN** a reader meets a declaration that the reader does not recognize
- **THEN** the reader SHALL preserve the record and SHALL treat the body as opaque

#### Scenario: A recipient list is additive
- **WHEN** a `v: 1` reader that predates this convention meets a record sealed to recipients
- **THEN** it SHALL preserve the record and treat the body as opaque, exactly as for any encoding it
  does not implement

### Requirement: A reader preserves what the reader cannot open

T2.3 ALWAYS, a reader SHALL preserve a record that the reader cannot open.
T2.3 ALWAYS, a reader SHALL rewrite no stored record.

#### Scenario: A sealed record survives a read
- **WHEN** a client reads a sealed record without the key
- **THEN** the stored record SHALL stay unchanged

#### Scenario: The bus file holds no plaintext body
- **WHEN** an operator searches the bus file for a known message body that was sealed
- **THEN** the search SHALL find no match

### Requirement: Fixtures cover an encoded body

T5.1 ALWAYS, the fixtures SHALL hold a sealed body.
T5.1 ALWAYS, the fixtures SHALL hold a tampered body.
T5.1 ALWAYS, the fixtures SHALL hold one stream with a plaintext body and a sealed body.

#### Scenario: Both clients read the fixtures
- **WHEN** either client runs the fixtures
- **THEN** both clients SHALL agree on every encoded case

### Requirement: A record may carry a signature

A record MAY carry a signature in `meta.sig`. The signature SHALL name the algorithm and the public
key that produced it, and SHALL cover the record's canonical sender, its canonical address, and its
body. A record without `meta.sig` SHALL remain a well-formed record.

#### Scenario: A signed record is well formed

- **WHEN** a client publishes a signed record
- **THEN** the record SHALL carry `meta.sig` naming the algorithm, the public key and the signature

#### Scenario: An unsigned record is well formed

- **WHEN** a client publishes a record with no signature
- **THEN** the record SHALL be accepted by the envelope rules exactly as before

#### Scenario: The signature field is additive

- **WHEN** a `v: 1` reader that does not verify meets a signed record
- **THEN** it SHALL preserve `meta.sig` and SHALL read the record

### Requirement: The signed bytes are canonical and fixed

The bytes a signature covers SHALL be a deterministic encoding of the canonical sender, the
canonical address, and the body as stored. Two spellings of the same sender or address SHALL produce
the same signed bytes. The signed bytes SHALL NOT include any field that the bus may rewrite.

#### Scenario: Two spellings sign the same

- **WHEN** the same sender and address are spelled two ways that canonicalize to one form
- **THEN** a signature over one SHALL verify against the other

#### Scenario: A replayed signature does not move

- **WHEN** a signature made for one sender and address is presented with a different sender or
  address
- **THEN** the signature SHALL fail to verify

#### Scenario: A rewritten sender is caught

- **WHEN** a stored record's sender differs from the sender the signature covers
- **THEN** a verifying reader SHALL refuse the record

### Requirement: The signature is readable without the body

A reader SHALL read `meta.sig` without opening the body, and SHALL verify the signature without
requiring the body to be readable as content. A sealed body SHALL be signed as stored bytes.

#### Scenario: A sealed body is verifiable

- **WHEN** a reader holds no key for a sealed body but holds the sender's anchor
- **THEN** the reader SHALL still reach a verdict about the signature

#### Scenario: The envelope stays readable

- **WHEN** a reader that can verify meets a signed record
- **THEN** the sender, the address, the offset and the signature SHALL be readable without the body

### Requirement: An unimplemented algorithm is not trust

A reader that does not implement the algorithm a record names SHALL report the record as unverified
and SHALL NOT report it as trusted or as refused.

#### Scenario: An unknown algorithm is tolerated

- **WHEN** a record names a signature algorithm the reader does not implement
- **THEN** the reader SHALL deliver the record as unverified and consumption SHALL continue

#### Scenario: A new algorithm needs no version bump

- **WHEN** an algorithm is added to the vocabulary
- **THEN** `v` SHALL remain unchanged and readers of the previous version SHALL keep reading

### Requirement: Signed and unsigned records share one stream

A stream MAY hold both signed and unsigned records. A reader SHALL apply its verdict rules per
record, and SHALL NOT treat the presence of unsigned history as making a later signed record
untrusted.

#### Scenario: Mixed history is per record

- **WHEN** a stream holds an unsigned record and a signed record
- **THEN** each SHALL receive its own verdict

#### Scenario: History is never rewritten

- **WHEN** a reader meets unsigned history on a strict bus
- **THEN** the reader SHALL refuse to read it unless the bus permits it, and SHALL rewrite no stored
  record

### Requirement: A record advertises its sender's agreement key

A record MAY carry an agreement key in `meta.sig.agree`: a public key of a second kind, used only to
agree a content key with a sender. It SHALL be inside the bytes the signature covers, so a substituted
key fails verification. It SHALL NOT be used to verify a signature.

#### Scenario: The key travels with the signature
- **WHEN** a client signs a record
- **THEN** the record MAY carry `meta.sig.agree`, and the signature SHALL cover it

#### Scenario: One key, one purpose
- **WHEN** a reader verifies a signature
- **THEN** it SHALL use the signing key only, and SHALL NOT accept an agreement key in its place

#### Scenario: An absent agreement key is not an error
- **WHEN** a signed record carries no `meta.sig.agree`
- **THEN** the record SHALL be readable, and the sender SHALL be treated as reachable only unsealed
