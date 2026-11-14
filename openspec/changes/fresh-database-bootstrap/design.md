# Design

## Context

`cli/src/pg/schema.rs` runs `BOOTSTRAP` then `AUTHORIZATION` in one `batch_execute`. `BOOTSTRAP`
creates `msgq.meta`, `msgq.address`, `msgq.cursor` and `msgq.retention`; `msgq.identity` is created
inside `AUTHORIZATION`, after a migration block that already queries `msgq.identity` and
`msgq.cursor`. One transaction, so the failure leaves nothing behind.

## Goals / Non-Goals

**Goals:** a single connection turns an empty database into a working bus. The zero-identity bus
behaves as `db info` describes it.

**Non-Goals:** migrating a database written by a newer build in the other direction; changing what a
credential means.

## Decisions

### 1. Tables before migrations, always

`msgq.identity` moves into `BOOTSTRAP`, beside the other three, so the table layer is complete before
any block reads it. The migration block stays in `AUTHORIZATION` and becomes pure data movement.
Alternative rejected: a conditional wrapper around the block, which hides the ordering problem instead
of fixing the order.

### 2. A credential is required only when one exists

Each read function resolves the caller the same way: if the token is empty **and** `msgq.identity` is
empty, the call proceeds as an unauthenticated caller with no scopes; otherwise the token must resolve.
That is the read-path mirror of what `publish_as` already does, and it keeps the no-downgrade rule: the
moment one identity exists, an empty token is refused.

### 3. The regression test creates its own database

`cli/tests/pg.rs` gains a case that `CREATE DATABASE`s a scratch name, bootstraps, reads, publishes,
then drops it. It is the only test whose starting state is nothing, which is what the bug needed.

## Risks / Trade-offs

- **Two schema sources** (Rust `BOOTSTRAP` and the SQL file) → the test asserts both run on an empty
  database, and the next step (a version guard) collapses them further.
- **An unauthenticated read sees every address** → true only while no identity exists, which is the
  single-user bus the README documents; the first registered identity closes the door.
