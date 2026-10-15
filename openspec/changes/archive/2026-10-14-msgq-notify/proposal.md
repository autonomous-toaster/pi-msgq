## Why

An agent can ask a peer a question, but only by standing still: `ask` occupies the session for as
long as the answer takes. The same is true outside the session — a script that runs for twenty
minutes has no way to say it finished without someone watching it. And the bus already delivers a
message into a session and can start a turn, so the missing piece is not delivery.

It is *what* an arriving message answers. A reply and an unsolicited prompt are indistinguishable:
both are a record of type `prompt` from another address. A session woken by an answer cannot tell
which question it answers, so it cannot safely stop waiting. Give a record a name, and let a record
say which record it is about, and a session can be woken usefully instead of merely being
interrupted.

## What Changes

- Give a record an id: `<address>#<offset>` — the address it was published to, and the offset it
  landed at. Both halves are already in the record and in what `publish` prints, so this is a name
  for something every record already has, not a new field. `publish` can report it, and
  `--reply-to` accepts it alongside a bare offset.
- Annotate references. When a delivered record carries `reply_to`, the extension resolves the record
  it refers to and states it, in the content the model reads: `answers session/reviewer#7: "review
  this diff"`. One rule covers both a reply and an acknowledgement, because both reference the same
  way — so "the answer arrived" and "they finished without answering" come from the same lookup.
- Keep it stateless. No table of outstanding questions, no timer, no deadline, no memory of what
  this session asked. The annotation is resolved at delivery, by reading the referenced record.
- Ship a skill, `msgq-notify`, inside the extension and advertised by it through its manifest,
  teaching how a process outside the session — a script, a background job, a CI hook, a watcher —
  reaches the session, wakes it, asks it something, and reports when work finishes.

Explicitly not in this change:

- No deadlines and no timers. Silence stays silent: nothing will tell you a peer never answered.
  The skill says so, and says to ask for an acknowledgement if you need to know.
- No record of what was asked, and no roster of who was asked. The extension remembers nothing
  between records.
- No minted identifiers: no `uuid` field, no new record field, no protocol version bump, and no new
  dependency. The id is derived, so it applies to every record ever published.
- No enforced identity for a background job, and no `job/` address scheme. The agent chooses its
  sender and the skill instructs; the trust list remains the operator's decision.
- No change to `ask`, and no new verb for resolving an id. Blocking is still right when you are
  blocked anyway, and `read` can already fetch one record.

## Capabilities

### New Capabilities

None. The id is a CLI behavior and the annotation is the extension's, so both belong to the
capabilities that already own them. The shipped skill adds no behavior of its own beyond being
advertised.

### Modified Capabilities

- `msgq-cli`: add the record id — reported by `publish`, accepted by `--reply-to` — while the bare
  offset keeps working.
- `msgq-extension`: annotate any delivered record that references another, and advertise the
  `msgq-notify` skill the extension ships.

## Impact

- No new deliverable. `extension/` gains a `skills/` folder and a `pi` manifest, and the extension
  loads exactly as before otherwise. The manifest must re-declare the extension entry, or adding
  the skill silently stops the extension loading.
- No record schema change, no protocol version bump, no new record type, and no new dependency. The
  id is derived, so it is valid for records published before this change.
- `publish` keeps printing the bare offset by default, so scripts that parse its output are
  unaffected; the id is opt-in.
- The annotation costs one read per referencing record, and the skill's description rides in the
  system prompt of every session that loads the extension.
