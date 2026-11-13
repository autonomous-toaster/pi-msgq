# Design

## Context

The shared engine authorizes writes through one function and nothing else. Reads, catalog listings,
cursor writes, prune and the retention watermark are direct statements on shared tables, so a caller
that can open a connection reaches every tenant's state. The identity row is keyed to an address, which
makes a user and a sender the same thing and forces every session to borrow the user's credential
without any way to tell the two apart afterwards.

This change keeps the shape that already works — one authorizing function, a runtime role that holds no
data privileges — and extends it to the rest of the surface.

## Decisions

### 1. Identity is a principal

A row names a user. The scopes are the addresses that user reaches, so a session addresses itself inside
the user's namespace without needing a credential of its own. This is what the original sketch meant by
`msgq.identity(user, ...)` and what the implementation lost when it keyed the row to an address.

Consequence: the sender of a stored record is a session, the identity of a stored record is a user, and
the two are now separate fields rather than one borrowed name.

### 2. Derive what the caller cannot be trusted to name, validate what the caller must name

| key | who supplies it | what the bus does |
| --- | --- | --- |
| address | the caller | validate against the scopes |
| sender | the caller | keep inside the scopes, refuse outside |
| cursor owner | nobody | stamp from the credential |
| cursor name | the caller | accept, scoped by the stamped owner |
| retention watermark | nobody | written by the prune that removed the records |
| writing principal | nobody | stamp in the headers |

Where a value has no reason to be caller-supplied, the bus derives it. An attack that requires forging
a derived value is not a refused attack, it is an impossible one.

### 3. Functions rather than row-level security

Row-level security needs a tenant identifier that the server trusts. A custom setting is not that: any
role may set a custom setting, so a client that holds the connection declares its own tenant and the
policy believes it. The alternatives are a database role per tenant — unbounded, and the write path
already rejected it — or one function per operation that resolves the credential itself. The function
surface is the same choice the write path made, so this keeps one architecture:

    msgq.publish_as(token, address, record)         -> offset
    msgq.read_since(token, address, since, limit)   -> records
    msgq.stream_of(token, address)                  -> head, oldest, dropped
    msgq.catalog(token)                             -> addresses, cursors
    msgq.cursor(token, address, name, offset?)      -> position
    msgq.prune(token, address, before, keep)        -> removed
    msgq.renew_credential(token, lease)             -> expiry

Presence is not in that surface. It is not bookkeeping the engine owns: it is derived from the presence
records a reader already sees through `read_since`, so a second derivation in SQL would be a second
answer to the same question.

Each resolves the credential first, checks the address second, and touches a table third. The runtime
role holds execute on these and no privilege on the streams, so the grants stop being decorative.

### 4. Owner columns rather than derived names

Namespacing cursors by concatenating the principal into the consumer name would change what a caller
passes, what the listing prints, and every existing row. An owner column does the same job and is
invisible: the key becomes (owner, consumer, address), the function stamps the owner, and a caller that
reuses another tenant's consumer name simply creates its own row.

### 5. Prune owns the watermark, and ownership outranks access

The watermark is a claim about what was removed, so only the operation that removes records writes it.
Prune authority follows ownership of the address rather than read access, which is the rule that keeps a
shared room from becoming a place where one member erases another's history. Today every address belongs
to one principal, so the rule is satisfied by construction; it is written down now because the teams case
would otherwise break it silently.

### 6. A lease renews, it never re-mints

Extending a credential by registering the principal again replaces the digest, which logs out every
other session of that principal — the sharpest edge in the current model for one user with several
sessions. Renewal extends the same row, keeps the digest, and is accepted after the lease elapsed while a
hard expiry has not passed, so a session resumed after a long idle period continues instead of being
locked out. Renewing when half the lease is spent keeps a live session from lapsing without a write
storm when several sessions share the row.

### 7. A claim outside the scopes is refused, not rewritten

The pgmq-backend change rewrites an out-of-scope sender to the identity's address. With a principal name
that rewrite has no address to produce, and a silent rewrite turns a mistake into a record that looks
legitimate. Refusing is louder and matches the boundary: a client may speak as itself, not as anyone
else. This narrows the earlier requirement. pgmq-backend has landed, so the reconciliation is in this
change's delta: the rewrite clause is removed and the stamping requirement now names the sender that
the scopes cover.

## Migration

- Identity rows keyed by an address become principals: the operator renames the row and keeps the scopes.
- Cursor rows predate the owner column: the single principal of an existing deployment is stamped on
  every row, or the cursors are reset and the retained window is re-delivered.
- Records written before the owner stamp carry no principal in the headers: a reader treats an absent
  value as unknown rather than refusing the record.

## Risks

- Renewal write amplification when many sessions share one identity row; mitigated by renewing at half
  the lease and by a greatest-of update.
- Catalog cost: listing visible addresses is a per-tenant query rather than a table scan, which is
  cheaper, but the function must not become a place where filtering is forgotten.
- The runtime role gains execute on more functions; each one is a place where the scope check must be
  present, so a test that calls every function cross-tenant belongs in the suite.

## What does not port

The local engine has no principal to verify and no server to enforce anything, so none of this applies.
Its tenancy model is one file per user, enforced by the operating system, and its trust statement already
says the sender is a claim.
