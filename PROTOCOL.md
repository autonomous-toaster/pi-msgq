# The msgq wire

What a client other than pi may depend on. This document is the contract: the CLI is its reference
implementation, the pi extension is its first client, and `protocol/fixtures/` is how the two are kept
honest.

Two words are used precisely here:

- **Promise** — a client written against this document keeps working. Changing it needs a `v` bump.
- **Convention** — a shape that shipped clients happen to agree on. It is documented so that new
  clients can join, but the wire does not enforce it, and a reader that ignores it still works.

## The envelope

A record is one JSON object. Four keys are required, four are optional, and nothing else is promised.

| key | required | meaning |
|---|---|---|
| `v` | yes | envelope version. This document describes `v: 1` |
| `type` | yes | a short string naming the sender's intent |
| `from` | yes | the sender's address |
| `to` | yes | the recipient's address; it is also the stream the record is written to |
| `message` | for message types | the body, as text |
| `event` | for `event` records | a structured body: an object |
| `reply_to` | optional | the offset of the record this one answers |
| `meta` | optional | an open object of hints for the receiver |

A record carries a body: `message` is text, `event` is an object. An `event` record requires `event`;
every other type requires `message`. Unknown top-level fields are **preserved** by writers that
rewrite records and **ignored** by readers.

## Types

`type` names what the sender wants to happen, not who consumes it. The set this version knows:

```
prompt  steer  follow_up  note  command  event
```

`type` is **not a closed list**. The known set is a convention: it is documented so that receivers can
interpret it, and it is *not* an allow-list enforced on publishers. A record whose `type` a reader does
not recognize is delivered as ordinary content; a record whose `type` a receiver cannot act on is still
a record. Adding a type is an additive change and keeps `v`.

## `meta`

`meta` is an open object. Two keys are conventions this version understands:

| key | shape | meaning |
|---|---|---|
| `ack` | `true`, or a comma-separated subset of `received,started,completed` | the sender asks the receiver to report lifecycle phases |
| `trace` | a string | a correlation identifier, so records belonging to one piece of work can be listed together |

`meta.trace` is how a delegation tree is reconstructed: a client stamps an identifier on the request
and every participant stamps the same identifier on what it sends in that work. A client that ignores
it loses nothing else.

Anything else a client puts in `meta` is preserved and ignored.
## Events

An `event` record carries an object in `event`. Two kinds are conventions:

### Acknowledgements

An acknowledgement is a normal record from the acknowledging address to the address that asked:

```json
{"v":1,"type":"event","from":"session/reviewer","to":"session/planner","reply_to":42,
 "event":{"kind":"ack","phase":"completed","delivery":"turn","outcome":"ok"}}
```

- `received` — responsibility was taken for record `reply_to`; `delivery` says `display`, `turn`, or
  `queued`.
- `started` — a run that includes it began.
- `completed` — that run ended; `outcome` says `ok`, `error`, or `aborted`.

Phases are requested with `meta.ack` on the original record. Acknowledgements are never answered, and an
acknowledgement is progress about another record rather than a message to reply to.

### Presence

Presence is a normal record published to the presence address:

```json
{"v":1,"type":"event","from":"session/planner","to":"group/presence",
 "event":{"kind":"presence","state":"idle","ttl":30,"participant":"01a187d8","alias":"planner"}}
```

- The presence address is `group/presence`. Every participant publishes there and the roster reads it;
  no session subscribes to it.
- A participant is live while its most recent presence record is younger than the `ttl` it declares, as
  measured by the **bus's stored timestamp** for that record — never by a clock the publisher claims.
- `participant` is the stable identity (the session id, or any stable name a non-session client
  chooses), `alias` the display name when there is one, and the record's `from` the address the
  participant answers at. The roster shows one entry per participant, because two sessions may share an
  alias and therefore an address.
- `state` is `idle`, `busy` or `blocked`.
- A clean shutdown publishes `state: "gone"`, which retracts presence immediately.
- Presence is advisory. It is not authentication, and it does not promise that a live session will
  answer. A client that publishes nothing at all still works; it simply is not listed.

## Addresses

An address names a stream and a recipient at once.

| scheme | for | who writes it |
|---|---|---|
| `session/<name>` | a live pi session, by alias or session id | the extension |
| `group/<name>` | a topic any number of participants may read | anyone |
| `cwd/<encoded-path>` | every session working in one directory | any launcher |
| `cli/<who>` | a process that is not a session: a script, a watcher, a CI job | anyone |

A bare name is shorthand for `group/<name>`. These four schemes are conventions about meaning; the
wire only requires the `publisher` to agree with the `to` field. A record's **id** is
`<address>#<offset>` and is derived, never stored.

## Streams, offsets and cursors

- Each address is a stream. Records are appended and keep their position.
- Offsets are monotonic **within a stream** and unique across the database, so a stream's offsets are
  sparse. An offset is the record's identity for `reply_to` and for ids.
- A consumer is a named position in a stream. Reading does not advance it; advancing is a separate,
  explicit act taken once responsibility for the record has been accepted. An interrupted consumer
  therefore redelivers rather than losing work — **at-least-once** is the promise, and idempotence is
  the receiver's business.
- A consumer that has never been seen starts at the **end** of the stream it follows: a first attach
  sees only new records, not retained history. `read` is the way to look at history.

## Compatibility

- Additive changes — a new `meta` key, a new event kind, a new type, a new address convention — keep
  `v` unchanged.
- A reader MUST accept records that carry fields it does not know, and MUST deliver records whose
  `type` it does not know as content.
- A reader MUST reject a `v` it does not implement, and MUST name the version it found in the reason.
- `v` changes only for a change a `v: 1` reader cannot survive.

## What is not promised

- **Delivery to a process.** The bus stores records; whether a session reads them is a property of
  that session's policy and trust list, not of the wire.
- **Acknowledgements or presence.** Both are conventions composed from ordinary records: a client may
  send them, ignore them, or never implement them.
- **Authentication.** `from` is a claim. The trust boundary is who can write the database file.
- **Retention.** Records stay until an operator removes them; no policy is implied.

## Conformance

`protocol/fixtures/` holds canonical records and, in `expectations.json`, what each implementation must
do with them. Both shipped implementations read the same file:

- `cli/src/record.rs` — the publisher's gate (what the CLI will write).
- `extension/src/record.ts` — the reader's tolerance (what a session will deliver).

Each entry names a `file`, whether `publish` and `read` accept or reject it, and optionally a `mentions`
substring the rejection reason must contain. A fixture that one side accepts and the other rejects is a
contradiction to resolve here, in this document, before it is resolved in code.
