## Context

See `proposal.md` for motivation and the two delta specs for the contracts.

What already exists, and what it constrains:

- A record is identified only by its **offset**, a number that means something different in every
  stream. `publish` prints it, `await <offset>` takes it, and `--reply-to <offset>` stores it.
- An acknowledgement is addressed to the record's sender and carries that record's offset as its
  reply reference — so an acknowledgement and a reply already have the same shape: a record that
  points at another.
- The extension subscribes, delivers, and acknowledges, and it writes only leases and
  acknowledgements. It holds no record of what this session has sent: the tools and the commands
  shell out to `msgq`, so nothing in the extension observes an outbound message.
- `formatMessage` renders the sender, the address it arrived on, and the body. It never mentions the
  reference, so a reply and an unsolicited prompt are identical to the model.
- The address grammar validates **only the scheme**: everything after `/` is accepted verbatim, so
  no character is guaranteed absent from an address.
- `herdr` is irrelevant here; nothing in this change touches it.

## Goals / Non-Goals

**Goals:**

- A delivered record can say what it is about, so a session woken later knows what it is answering.
- The identity of a record costs nothing: no new field, no version bump, and valid for records that
  were published before this change.
- No memory in the extension. Correlation is resolved, not remembered, so it works for a question
  asked from a shell, from a script, or from a session that has since restarted.
- The knowledge a process outside the session needs — how to reach it, wake it, and report — ships
  with the extension rather than living in a README nobody reads.

**Non-Goals:**

- Deadlines, timers, retries, and any notion of "expected by".
- A table of outstanding questions, or a roster of who was asked what.
- A minted identifier, a record field for it, or a protocol version bump.
- Enforced identity for a background job, or any new address scheme.
- Changing `ask`, which should keep blocking for the caller who is blocked anyway.

## Decisions

### D1. The id is derived, not minted

A record's id is `<address>#<offset>`: the canonical address whose stream holds it, and the offset
assigned there.

- Alternatives: a `uuid` field generated at publish; a `stream@offset` form; an opaque counter.
- Rationale: both halves are already known to both ends — the publisher gets the offset from
  `publish` and chose the address, and the receiver sees the same pair in the record it answers.
  Deriving it needs no field, no generation site, no dependency (there is no `uuid` crate in
  `cli/Cargo.toml`), and no version bump, and it is therefore retroactive: every record already
  published has an id. A minted id would add exactly two things — deduplication of a retried
  publish, and a reference that survives being copied into another stream — and neither is a
  present need. The derived form is forward-compatible with minting later, because a minted id
  would occupy the same position with a different spelling.
- Consequence: the id is a name for something the bus already had, and it is meaningless outside the
  bus that wrote it.

### D2. The separator is `#`, chosen for the reader

- Alternatives: `@` (see above), `:` (collides with lease names), and the first-character rules
  either would need.
- Rationale: the grammar guarantees nothing, since an address's name is unvalidated, so the choice
  is made on what occurs in practice and on what a model reads correctly. `@` occurs in real
  addresses — `cli/<user>@<host>` is the default sender — so `cli/alice@mac@7` would require the
  reader to split on the *last* `@`, a rule they can get wrong. `#` does not occur in addresses in
  practice, and `#7` already means "number seven" to a reader. `:` would collide with the lease
  naming already in use.
- Consequence: the one other place `#` appears, a consumer identity such as `planner#01a12f9d`, has
  no slash, so the two shapes remain distinguishable at a glance.

### D3. A reference is exact, and the writer is held to it

`reply_to` is an offset in the stream named by the referencing record's own `from`, so the record it
names is always `<from>#<reply_to>`. `--reply-to <address>#<offset>` therefore requires `<address>`
to be the record's own sender, and is a usage error otherwise.

- Alternatives: accept any id and store the bare offset (which would make the reference wrong for a
  foreign address, since the reader would look in the wrong stream); store the full id in a new
  field.
- Rationale: the invariant already holds for everything the system emits — a reply and an
  acknowledgement are both sent by the party the original was addressed to, so the original is in
  that party's stream — and this keeps it true rather than assumed. A full id field is the escape
  hatch if referencing a third party's stream is ever wanted, and it is a record-contract change
  that should be made deliberately, not pre-emptively.
- Consequence: you can only answer something that was addressed to you, which is what answering
  means.

### D4. One rule covers replies and acknowledgements

The extension annotates any delivered record that carries a reply reference, whatever its type.

- Rationale: an acknowledgement is a reply reference with a phase attached. Treating them
  separately would mean two mechanisms for one relation, and the failure the user actually feels —
  "they finished and never answered" — is a combination of the two: an acknowledgement that
  completed, and no answer.
- Consequence: no new record type, no new field, and no code that knows what an acknowledgement is
  in order to correlate it.

### D5. Stateless, with no deadline

No table of outstanding questions, no timer, and no sweep. The reference is resolved at delivery by
reading the referenced record.

- Alternatives: a map from id to what was sent, populated when the session's own tools publish; a
  timer that delivers "no answer yet" after a window.
- Rationale: a map only knows what this session sent through the tools, so it fails exactly where
  correlation is most valuable — a question asked from a shell, by a script, or before a restart.
  The lookup is stateless and covers all of those, and the extension already needs no memory for
  anything else on the delivery path.
- Consequence, and it is the honest cost: **silence stays silent.** Nothing will ever report that a
  peer did not answer. The skill says so in as many words, and says to ask for an acknowledgement
  when it matters. This is a deliberate trade: less mechanism, and the knowledge carries the rest.

### D6. The annotation belongs in the content, not the renderer

The reference is stated and the referenced record quoted in the text the model reads.

- Alternatives: the transcript renderer only, as the reply affordance does.
- Rationale: the two audiences are the point. The reply affordance is *for a human*, who needs to
  know how to answer, so it lives in the renderer and never reaches the model. A reference is *for
  the model*, which must know what it is being told about in order to act; a human reading the
  transcript already has the original in front of them. Same split, opposite direction, and worth
  stating because it is the opposite of the previous change.
- Consequence: the content grows by one line per referencing record, and the plain-text form of a
  delivered message is no longer independent of the record's history.

### D7. Resolving a reference is a read, and a read must not consume

The lookup reads one record and does not touch a cursor.

- Alternatives: subscribe to the referencing stream with a throwaway consumer.
- Rationale: a consumer leaves a row and moves an offset, and a read must not be able to
  acknowledge, redeliver, or otherwise alter what is pending. The bus already offers a read by
  offset; the extension's own wrapper needs a one-shot read added to it.
- Consequence: a record that is annotated is still pending until it is delivered, exactly as before.

### D8. The skill ships inside the extension, through the manifest

`extension/skills/msgq-notify/SKILL.md`, declared by a `pi` manifest in `extension/package.json`.

- Alternatives: a top-level `skills/` directory registered by the extension from the
  `resources_discover` event.
- Rationale: the manifest is declarative and needs no code, and `pi -e ./extension` already resolves
  a directory through the manifest, so the skill travels with the extension in development and when
  installed. The skill also remains loadable on its own with `--skill`, so nothing is lost by
  keeping it inside.
- Consequence, and it is a trap rather than a trade-off: **the manifest must re-declare the
  extension entry.** A manifest, or a `skills/` directory without one, makes the resolver treat the
  folder as a package and stop registering it as an extension, so a skill added carelessly would
  silently take the bus extension with it. Every end-to-end scenario starts with `pi -e`, so a wrong
  manifest fails the whole suite rather than hiding.

### D9. The skill teaches, and nothing is enforced

The skill covers reaching a session from outside: waking it, asking it, answering it, and reporting
when work finishes. It names no job identity and no address scheme.

- Alternatives: a `job/<name>` address scheme; a `cli/bg@<host>` convention; an allowlist entry
  written by the tooling.
- Rationale: the trust list already answers "who may wake me", and a background job is just another
  sender. Adding a scheme would be machinery for a naming problem, and the skill is where the
  knowledge belongs.
- Consequence: the first thing the skill must say is the trap a model will otherwise fall into —
  **a session cannot wake itself**, because a record whose sender is its own address never starts a
  turn. A job that quietly uses `--from session/<self>` will report into the void, and the failure
  looks exactly like a job that never finished.

### D10. `ask` is unchanged

- Rationale: blocking is right for the caller who is blocked anyway, and `ask` already ends on a
  reply, on a terminal acknowledgement, or on its window. This change makes the asynchronous path
  good, not the synchronous one redundant.
- Consequence: two ways to ask, and the skill says which is which.

## Risks / Trade-offs

| Risk | Mitigation |
|---|---|
| A missing manifest line silently disables the extension while shipping the skill | The manifest re-declares the extension entry, and every e2e scenario loads the extension with `-e` (D8) |
| The referenced record has been dropped or its stream removed | The annotation names the id without the quote, and delivery continues (D4) |
| One extra read per referencing record on the delivery path | One record, bounded, and a failure to read degrades to naming the id |
| An id is unauthenticated: anything that can write the bus can claim a reference | Consistent with acknowledgements, which are already unverified; the trust list is the operator's control, and the bus is one local file |
| A model quotes a foreign id and is refused | The refusal is a usage error naming the reason, and the id it should have used is the one it received |
| Silence is indistinguishable from a peer that was never asked | Stated in the skill; ask for an acknowledgement when it matters (D5) |
| The skill's description costs prompt space in every session | One line, and it is the price of the model knowing the pattern exists |

## Migration Plan

None required.

- Additive: no schema change, no protocol version bump, no new record type, and no new dependency.
- `publish` keeps printing the bare offset by default, so scripts that parse it are unaffected; the
  id is opt-in.
- The id is retroactive, so records written before this change are referenced like any other.
- Rollback is not using it: the annotation is a delivery-time read, and nothing is written.

## Open Questions

- A verb to resolve an id in one step (`msgq show session/reviewer#7`). `read --from <n-1> --limit 1`
  already works, so this is a convenience, and it can wait until the three flags annoy someone.
- Whether the reply affordance in the transcript should show an id as well as the sender, now that
  there is a name for the message a human is looking at.
- Whether the model-facing reply hint should be driven by `reply_to` rather than by `meta.reply`.
  They say different things — "the sender asked to be answered" versus "this record is about that
  one" — so merging them would be a loss, but the wording should be checked against real traffic.
- Whether a minted id is ever needed, which would be answered by a retry path or by a cross-machine
  bus, neither of which exists.
