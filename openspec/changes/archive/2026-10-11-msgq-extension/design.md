## Context

See `proposal.md` for motivation and `specs/msgq-extension/spec.md` for the behavior contract.

Three constraints from the existing system shape the approach:

- The bus already exists and is owned by the `msgq-cli` change: durable streams, monotonic
  per-consumer offsets, and a cross-process commit watcher over one SQLite file.
- pi's extension API already exposes exactly the delivery matrix this needs.
  `sendCustomMessage` has four cases — idle/streaming crossed with `triggerTurn` — and
  "streaming without a trigger turn" appends once the current turn ends, so display-only messages
  are safe even mid-run. `agent_before_settle` exposes a `BoundaryState` whose `entries` and
  `continue` allow appending one message and requesting exactly one more provider request.
- `@russellthehippo/honker-node` (0.5.1) implements the consumer side of the engine: a
  `StreamSubscription` async iterator that wakes on the shared watcher, returns already-parsed
  payloads, and saves offsets itself. Its default save cadence is 1000 events or 1 second.

## Goals / Non-Goals

**Goals:**

- Consume the bus from inside a session using the engine's own API, with no hardcoded schema and
  no polling timer.
- Make the safe path the default: an external message is display-only unless the subscriber
  explicitly grants more.
- Collapse bursts of turn-bearing messages into one continuation.
- Load with `pi -e <dir>` and require no installation.

**Non-Goals:**

- Publishing from the extension, replies, or agent-to-agent coordination.
- Presence or discovery publishing.
- Distribution through `pi install`, npm, or a package manifest.
- Any change to the CLI or the record contract.
- Retention, pruning, or cursor garbage collection.

## Decisions

### D1. Consume with the honker Node binding

Use `@russellthehippo/honker-node`, not raw SQLite access or a subprocess.

- Alternatives: `node:sqlite` reading `_honker_stream` directly; spawning `msgq follow` and
  parsing JSONL; the loadable extension via `loadExtension`.
- Rationale: it is the engine's own API on the consumer side, so the extension never hand-rolls SQLite access
  (with the single documented exception in D12), and the shared watcher wakes on any process's commit. It also
  removes the raw cursor upsert and JSON parsing. The subprocess option was rejected outright
  because it needs a resolvable binary path and lifecycle management for a child process.
- Cost: a native dependency. It ships statically linked prebuilt binaries per platform and needs
  `npm install` in the extension directory, which the `-e` loading model supports because pi
  resolves dependencies from a nearby `package.json`.

### D2. Read-only

The extension never publishes. Senders use the CLI.

- Rationale: it keeps this change strictly CLI-to-session and avoids the first step of
  agent-to-agent messaging, which is out of scope. The binding would make publishing trivial
  later, so this is a scope decision, not a limitation.

### D3. Subscriber policy caps publisher intent

The record's `type` is the publisher's request; the subscriber's policy is the authority.
`display` (default) delivers everything as display-only. `honor` lets `type` act on the session.

- Alternatives: honor `type` unconditionally; a per-topic policy map; a numeric trust level.
- Rationale: a `prompt` record is remote control of a process with shell access, so the default
  must be inert. A single per-session policy is enough for v1; per-topic policy is a later
  refinement and would change the spec, so it is deliberately not built now.
- Consequence: publisher intent can only ever be downgraded, never escalated.

### D4. Coalesce at the settle boundary

Buffer turn-bearing records that arrive during a run; deliver them as one message and set
`continue: true` once at `agent_before_settle`.

- Alternatives: deliver each record as its own queued turn; use `deliverAs` per record.
- Rationale: N records must not become N model requests. `agent_before_settle` is the documented
  boundary for appending entries and requesting one continuation.
- Risk to manage: an unconditional `continue` can loop. The continuation is requested only when
  the buffer is non-empty and is cleared before delivery, so at most one continuation per batch.

### D5. Consumer identity is derived and unique

Consumer id is the session alias joined with the session identifier when an alias exists,
otherwise the session identifier alone.

- Alternatives: the alias alone; the session id alone.
- Rationale: the alias alone collides across sessions and would make two sessions share a cursor
  and lose messages. The id alone is unique but opaque in `msgq consumers`. Combining them is
  both unique and readable.

### D6. Default subscription plus opt-in extras

Always subscribe to `session/<self>`; additional addresses come from a flag and an environment
variable, both comma-separated.

- Alternatives: a configuration file; subscribing to `cwd/<enc>` by default.
- Rationale: the session's own address is the minimum useful subscription and needs no
  configuration. Everything else is explicit, which keeps the trust surface small and the
  behavior predictable. A config file is unnecessary while there is one list and one policy.

### D7. Save the cursor before reporting

Subscribe with the save cadence collapsed to a single event (`saveEveryN: 1`, time-based saving
disabled).

- Alternatives: the default 1000 events or 1 second.
- Rationale: the binding increments its delivered counter and calls the save routine before
  returning the event, so this setting yields exactly "persisted before reported". The cost of
  one small write per message is negligible at human and script message rates.

### D8. Lifecycle tied to the session, not the factory

Create the engine handle and subscriptions on `session_start`; close them on `session_shutdown`.

- Rationale: the extension contract forbids starting processes or watchers in the factory, and
  reload replaces the runtime. Closing both the subscriptions and the engine handle releases the
  watcher thread and file handles, which matters on platforms that cannot unlink an open file.

### D9. Raise the watcher cadence

Open the engine with a raised watcher poll interval rather than the 1 ms default.

- Alternatives: the default 1 ms.
- Rationale: 1 ms is tuned for lowest-latency workers, not an idle interactive session. A few
  hundred milliseconds trades imperceptible latency for lower idle CPU. Message latency is not
  user-visible at that scale.

### D10. Package as a local directory

`extension/index.ts` plus `extension/package.json` declaring the binding; loaded with
`pi -e ./extension`.

- Alternatives: a pi package with a manifest; a single file with no dependencies.
- Rationale: a single file is impossible now that there is a native dependency, and installing
  or publishing is explicitly out of scope. A nearby `package.json` is the documented way to
  give an extension its dependencies.

### D11. Keep the engine generation aligned

The CLI pins honker 0.5.0 and the binding is 0.5.1. Treat them as one generation.

- Rationale: both sides read and write the same file, and the binding documents cross-binding
  schema compatibility. Bumping one without the other is a two-sided change and should be treated
  as such.

### D12. Initialize a new consumer at the head

Before subscribing a consumer that has no cursor row, issue one indexed head-offset query through
`db.query` and save that offset as the consumer's starting point.

- Alternatives: let the binding start a new consumer at offset 0 (its default); drain the backlog
  through the public API and discard it; cap or replay the backlog deliberately.
- Problem: the binding exposes no head-offset API, and `getOffset` returns 0 both for a missing
  row and for a row at 0. Subscribing a new consumer therefore reads a topic's entire retained
  history. Injecting a whole backlog into a session is unsafe — under `honor` it could start a
  turn per record.
- Rationale: one indexed `MAX(offset)` query is cheap and makes first attach deliver only new
  records, while a consumer that already has a cursor still resumes from it. This is the same
  query and the same table the CLI's `topics` command already reads, so the engine-table access
  is a known, contained exception rather than a second source of schema truth.
- Note: this is the only place the extension reads an engine table directly.

### D13. A dedicated renderer for msgq messages

Register a message renderer that composes a structured transcript entry from per-message
`details`, and keep the text composition in a module with no runtime imports.

- Alternatives: leave msgq messages as generic custom messages; format inside `content`.
- Rationale: `content` is what non-interactive modes consume and what the model sees, so it must
  stay plain text. `details` is not sent to the model, which makes it the right place for the
  fields the transcript wants (sender, address, type, body). Registering the renderer in the
  factory is safe because it allocates no resources.
- Testability: splitting the pure composition from the TUI component lets the layout be asserted
  with a stub theme under `node --test`, which cannot resolve the host TUI package.

## Risks / Trade-offs

| Risk | Mitigation |
|---|---|
| Native binding fails to resolve or interop with the TS loader | Spike a minimal `-e` extension that imports and opens the bus before building the rest; fall back to a `createRequire` import if ESM interop is awkward |
| The `.d.ts` advertises a `signal` option that 0.5.1's implementation does not use | Cancel by calling `close()` on the subscription and the engine handle; do not rely on the signal |
| `close()` may not unwind the pending wait instantly | Accept up to the fallback poll interval to stop; cleanup is idempotent so a late unwind is harmless |
| The watcher can report itself dead (for example if the file is replaced) | Treat it as graceful degradation: stop consuming and report, do not crash the session |
| A 0.4.6-era checkpoint can raise an unverifiable-progress error | Surface it and document the explicit recovery (re-establish a canonical offset) rather than guessing |
| An external message can drive an agent with shell access | `display` is the default; `honor` is explicit opt-in; provenance is always in the delivered message; self-addressed records never start a turn |
| A continuation loop | Continue only when the buffer is non-empty, and clear it before delivery |
| Many subscriptions multiply watcher load | Subscriptions share one watcher per engine handle; the raised cadence bounds idle cost |
| Two sessions with one alias share a cursor | Consumer identity combines alias and session id (D5) |
| A renderer that assumes structured details breaks messages that lack them | Fall back to the plain `content` when `details` is absent |

## Migration Plan

New component; no data or schema migration.

- Enable by starting pi with `-e ./extension`; disable by dropping the flag.
- The bus is unchanged, so a session can start and stop consuming without affecting other
  sessions or the CLI.
- Cursors created by the extension are ordinary consumer rows; removing them is a CLI concern
  (`msgq consumers rm`), not a schema change.

## Open Questions

- The exact watcher interval value: a few hundred milliseconds is chosen at implementation time
  and does not change the specs.
- Whether to add a user-facing `/msgq` command to show subscription status, or rely on a status
  line and `ctx.ui.notify`. This affects presentation only.
- Whether a future `trust` policy (everything becomes a turn) is ever warranted, or whether
  `display` and `honor` are sufficient. Adding it later is additive.
- Per-topic policies. Deliberately deferred; adding them changes the policy requirement and
  belongs in its own change.
