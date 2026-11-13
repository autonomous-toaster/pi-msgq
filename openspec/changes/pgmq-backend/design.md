# Design

## Context

See `proposal.md` for motivation, and `specs/msgq-backend/spec.md` for the behavioural contract. The
constraints that shape the approach are these:

- The current storage layer is one concrete type over honker's durable streams: a single
  `_honker_stream(topic, offset, payload, created_at)` table, a `_honker_stream_consumers(name,
  topic, offset)` cursor table, an advisory lock, and an in-process update waker. `store.rs` confines
  all engine use, but the pi extension opens the same file directly and runs raw SQL against those
  two tables (`extension/src/consumer.ts`), so there are already two implementations of the access
  path.
- pgmq's primitives were read from `pgmq-extension/sql/pgmq.sql` on `main` rather than from its
  README. Four facts are load-bearing and each is quoted from that source below.
- The bus is a *log with cursors*, not a queue. pgmq is a queue. The design is therefore an explicit
  mapping, not an adoption of pgmq's API.

## Goals / Non-Goals

**Goals**

- One behavioural contract, two engines, chosen once per deployment.
- Keep every existing wire promise that can survive a shared database.
- Make a shared bus usable by several users without trusting a claim.
- Make retention an explicit, short, configurable window on both engines.

**Non-Goals**

- No message encryption (only the seam that makes it a follow-on change).
- No claim/work-sharing delivery verb; delivery stays broadcast.
- No relay, federation, or data migration between engines.
- No attempt to defeat a malicious database operator beyond keeping content opaque.

## Decisions

### 1. A narrow store trait, two implementations

Eleven methods carry everything: `ensure_address`, `publish`, `read_since`, `head`, `tail`,
`cursor_get/set/reset`, `prune_before`, `list_addresses`, `list_consumers`, `wake`. Everything above
it — record shape, address grammar, presence parsing, ack matching, policy — is engine-independent,
which is why the SQLite deployment inherits retention, gap reporting and (later) encryption for free.

*Alternatives.* Replacing honker outright: destroys the "no daemon" default. Two binaries: doubles
the CLI surface. A wider trait carrying SQL: leaks the engine into `commands.rs`, which is exactly
what `store.rs` exists to prevent.

### 2. pgmq owns storage and identity; it does not own the read path

Used: `pgmq.create` (lazy, idempotent), `pgmq.send`, `pgmq.delete` (batched, for retention),
`pgmq.meta` (address enumeration), the insert trigger for wake, `pgmq.metrics` (health).

Not used on the delivery path: `pgmq.read`, `pgmq.set_vt`, `pgmq.archive`.
`read` is a lease: it sets `vt`, bumps `read_ct`, and stamps `last_read_at` — it makes a record
invisible to everyone else, which a per-consumer cursor must never do. `archive` moves a row to a
permanent `a_<queue>` table, which is a retention anti-pattern when the window is an hour.

*Rejected: fan-out through `pgmq.send_topic` + `bind_topic`.* `send_topic` loops over matching
bindings and `RETURN matched_count`; with no binding the record is written **nowhere**, and each copy
is minted a *different* `msg_id`. Publishing before anyone subscribes is normal on this bus, and one
record with N offsets has no single `reply_to`. So fan-out happens at the cursor layer, never at
storage.

### 3. One queue per address, plus a cursor table

pgmq has no per-consumer position on a shared queue, and per-consumer queues are impossible because
the bindings do not exist at publish time (decision 2). So the cursor is ours:

```
msgq.address(address PK, queue_name UNIQUE, created_at)
msgq.cursor(consumer, address, last_msg_id)        -- monotonic, never advanced by a read
```

Reads are `SELECT ... FROM pgmq.q_<name> WHERE msg_id > $1 ORDER BY msg_id LIMIT $n` — read-only,
no lease, no counter, so `msgq read` stays idempotent and `follow` keeps its
"save after responsibility" semantics unchanged.

### 4. Queue names are derived, never passed through

`pgmq.format_table_name` returns `lower('q_' || name)`, and pgmq's own trigger source says why that
matters: *"The table name cannot be turned back into it: format_table_name lowercases, so the queue
MyQueue lives in pgmq.q_myqueue"*. `validate_queue_name` caps the name at 47 characters. Pass-through
would silently merge `session/Api` with `session/api` and reject any `cwd/` address.

So: `queue_name = <slug>_<16 hex of sha256(address)>`, capped well under 47. The slug is cosmetic
legibility in `\dt pgmq.*`; the digest is the identity. The true address is stored twice — in
`msgq.address` (so an empty queue still has a name) and in each record's `headers` (so a message is
self-describing even if the registry is lost).

*Alternative.* Hash-only names: simpler, but throws away half of why a team wants pgmq.

### 5. Offsets become per-stream, deliberately

pgmq allocates `msg_id` with `GENERATED ALWAYS AS IDENTITY` **per queue table**, so there is no
global offset space. The wire's "unique across the database" was an honker property that leaked into
the contract. Identity becomes `(address, offset)`, which is what every id already spelled.

*Rejected.* Re-pointing every queue's identity at one shared sequence with
`ALTER TABLE ... DROP IDENTITY; SET DEFAULT nextval(...)`: it works, and it is exactly the vendor
fork the standard-first constraint forbids.

### 6. Retention is a time-based prefix delete

`pgmq.create_partitioned(queue, partition_interval, retention_interval, premake)` looks like the
answer but pg_partman's retention axis here is **msg_id count**, not time: a quiet address keeps
`retention_interval` messages for years, a busy one loses them in days. So: plain queues, and
`msgq gc` deletes `WHERE enqueued_at < now() - window` in batches through `pgmq.delete(queue,
msg_ids[])`, with `pgmq.delete(queue, msg_id)` for single rows.

Two things pgmq does not give us and this change must add: an index on `enqueued_at` (the queue table
indexes `msg_id` only, plus a GIN on `headers` for FIFO), and a place to record the window.

### 7. Authorization is one function, not a grant per queue

pgmq's functions are caller-privileged (`SET search_path`, no `SECURITY DEFINER`), so per-user
isolation via grants would mean `GRANT` on every `q_*` table — unbounded, and every participant could
read every queue. Instead:

```
msgq.identity(user, token_digest, write_scopes, read_scopes, expires_at)
msgq.publish(to, record, token)        SECURITY DEFINER, SET search_path = pg_catalog, pg_temp
```

The function resolves `digest(token,'sha256')` (minted with `gen_random_bytes(32)`), checks `to`
against the identity's scopes, stamps `from` from the identity, creates the queue if absent, and
calls `pgmq.send`. The runtime role gets **no** `CREATE` on schema `pgmq` and no `INSERT` on `q_*`,
so the function is the only write path — otherwise enforcement is decorative.

The local engine has no counterpart and needs none: its trust statement stays file permissions.
Credentials live in the participant's identity file (`0600`, beside the db) or `MSGQ_TOKEN`, and are
never placed in a record.

### 8. Wake is pgmq's own insert trigger

`pgmq.enable_notify_insert(queue, throttle_ms)` installs a trigger that emits
`PG_NOTIFY('pgmq.q_<name>.<OP>', NULL)` under a per-queue throttle. Payload is `NULL` by design: the
wake says "read now", not "here is the record", which is the right shape for a log. Polling remains
the fallback for connections that cannot `LISTEN`.

### 9. The extension gets its own client, not a subprocess

Its consumer needs per-turn latency; spawning the CLI per poll would put a process spawn in the turn
path. A second bus implementation in TypeScript is the same sin the project already commits (Rust and
TS both read honker's tables), so it is accepted deliberately, with queries kept in one shared place
so the two cannot drift.

### 10. The encryption seam is a shape, not a feature

Content is never read by server-side SQL, so a future encryption pass changes only the client. That
invariant is what makes the follow-on cheap, and it is why `meta.enc` is reserved now. Three code
paths read *body* fields today, and all three are bodies that must stay readable forever: presence
parsing (`presence.rs`), the newest-presence-per-participant query (`store.rs`), and ack matching
(`commands.rs::await_terminal`). Content may become opaque; those may not.

### 11. Retention keeps a watermark, so a lagging consumer learns what it lost

Retention deletes rows, so after a prune there is nothing left to count. Every prune therefore
appends `msgq.retention(topic, cut_offset, removed_total, removed_at)`, carrying the cumulative number
of records removed for that topic up to `cut_offset`. Three consequences:

- `msgq consumers` reports `dropped = latest_total(topic) - total_at_or_before(cursor)`, which is exact
  whenever a consumer's cursor sits on a prune boundary and an upper bound otherwise (a cursor inside
  the last pruned batch counts that whole batch). The bound is honest: those records are gone either
  way, and over-reporting a loss is safer than hiding one.
- `read` and `follow` report the same number when they find a position below the oldest retained
  record, and `--strict` turns a truncated read into a non-zero exit.
- `db info` can report the last prune per topic without a scan of the engine's tables.

*Rejected.* Storing the offsets of removed records: retention exists to make them unrecoverable.
*Rejected.* Reporting `oldest_retained - cursor`: offsets are sparse, so the number would look exact
and be wrong.

## Risks / Trade-offs

- **[DDL on the publish path]** First publish to an address creates two tables. → Idempotent and
  cached in `msgq.address`, executed outside the record's transaction, with `pgmq.acquire_queue_lock`
  serializing concurrent creators. Cost is milliseconds at human speed.
- **[Autovacuum pressure]** A short window means frequent deletes across many tables, and
  `autovacuum_max_workers` defaults to 3. → Batched deletes, an `enqueued_at` index, and per-table
  dead-tuple monitoring in `db info`.
- **[Coupling to pgmq's schema]** History and tail reads are plain SELECTs against `q_*`, which pgmq
  does not promise. → Record the pgmq version in the bus metadata, report it in `db info`, and refuse
  a version the build has not been tested against, so a schema change fails loudly.
- **[Pooler breaks LISTEN]** Transaction-mode pooling does not keep `LISTEN` alive. → Polling remains
  the guaranteed path; the push path is an optimization that degrades quietly.
- **[Short window loses records]** An offline consumer silently falls behind. → Gap counts in
  `read`, `follow`, `consumers` and the extension; `--strict` for scripts.
- **[`await` loses its bare offset]** A CLI surface break for anyone scripting `msgq await 42`. →
  Record ids (`session/x#42`) work everywhere an offset does; document it in the README migration
  note.
- **[Two engines drift]** Nothing structural prevents it. → Behavioural fixtures are the only defence,
  and they are part of the change rather than a follow-up.

## Migration Plan

1. Ship the backend-neutral half first (wire amendments, retention window, gap reporting) on the
   local engine only. No shared database is needed to test it, and the short window starts paying for
   itself immediately.
2. Add the shared store and the authorizing publish function; a deployment opts in by pointing
   `MSGQ_DB` at a connection string.
3. Ship the extension's second bus, keeping polling as the guaranteed path.
4. Rollback is configuration: point `MSGQ_DB` back at a path. There is no data migration in either
   direction, by design — the two engines never share records.

Existing local databases need no schema change; the default window (1 hour) changes `prune`'s
behaviour, which is the intended break and is reported by `db info`.

## Open Questions

- The minimum pgmq version to accept. Deferrable: `db info` can report the installed version and the
  build's floor can be set once a first shared deployment exists.
- Whether `msgq gc` should run on a timer inside the extension or be left to the operator (systemd
  timer, cron, or a Postgres scheduled job). Deferrable: the command is the same either way.
- Which PostgreSQL client the extension uses. Deferrable: both candidates expose the same
  listen/query surface the bus needs.
