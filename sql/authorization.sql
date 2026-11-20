-- Authorization for the shared engine.
--
-- The rule this file exists to enforce: a runtime role may not write to a stream directly. It may
-- only call msgq.publish_as, which resolves a credential, checks the address against that
-- identity's scopes, stamps the sender, and then writes. Every function that writes is
-- SECURITY DEFINER with a search_path fixed to this file's own dependencies, so a caller cannot
-- redirect a lookup by setting a schema.
--
-- The credential itself is never stored: a row keeps sha256(token) and the token exists only in the
-- identity file or MSGQ_TOKEN. Nothing here returns a token except register_identity, which mints
-- one for an operator to hand out.

-- pgcrypto holds gen_random_bytes and digest. Managed services usually provide it already, in
-- public or in an extensions schema; this statement only helps a plain server.
CREATE EXTENSION IF NOT EXISTS pgcrypto SCHEMA public;

ALTER TABLE IF EXISTS msgq.address ADD COLUMN IF NOT EXISTS owner TEXT NOT NULL DEFAULT '';
ALTER TABLE IF EXISTS msgq.cursor ADD COLUMN IF NOT EXISTS owner TEXT NOT NULL DEFAULT '';

DO $$
BEGIN
    IF EXISTS (SELECT 1 FROM msgq.cursor)
       AND NOT EXISTS (SELECT 1 FROM information_schema.key_column_usage
                       WHERE table_schema = 'msgq' AND table_name = 'cursor'
                         AND constraint_name = 'cursor_pkey' AND column_name = 'owner') THEN
        ALTER TABLE msgq.cursor DROP CONSTRAINT cursor_pkey;
        ALTER TABLE msgq.cursor ADD PRIMARY KEY (owner, consumer, address);
    END IF;
    -- One user on this bus: the addresses it already wrote are its own.
    IF (SELECT count(*) FROM msgq.identity) = 1 THEN
        UPDATE msgq.address
           SET owner = (SELECT name FROM msgq.identity LIMIT 1)
         WHERE owner = '';
    END IF;
END;
$$;

CREATE TABLE IF NOT EXISTS msgq.identity (
    name TEXT PRIMARY KEY,
    digest BYTEA NOT NULL UNIQUE,
    scopes TEXT[] NOT NULL,
    expires_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

COMMENT ON TABLE msgq.identity IS
    'One row per writer: the sha256 of a credential, the addresses it may write, and an optional expiry.';

-- Recreating the write path is how a schema change lands: it is code, not data, and a CREATE OR
-- REPLACE cannot rename an argument. The grants are re-applied at the bottom of this file.
DROP FUNCTION IF EXISTS msgq.register_identity(TEXT, TEXT[], TIMESTAMPTZ);
DROP FUNCTION IF EXISTS msgq.publish_as(TEXT, TEXT, JSONB);
DROP FUNCTION IF EXISTS msgq.identity_of(TEXT);

-- The canonical spelling of an address: whitespace and stray slashes removed, runs of slashes
-- collapsed, case preserved. Case is part of the address, as it is in the CLI: the shared engine
-- does not quietly lowercase a name the local engine keeps.
CREATE OR REPLACE FUNCTION msgq.canonical_address(address TEXT)
RETURNS TEXT
LANGUAGE sql IMMUTABLE
SET search_path = pg_catalog, pg_temp
AS $$
    SELECT regexp_replace(btrim(address, ' /'), '/+', '/', 'g')
$$;

-- A scope is an address and everything beneath it. "group/red" covers "group/red/sub", not
-- "group/redder".
CREATE OR REPLACE FUNCTION msgq.in_scope(address TEXT, scopes TEXT[])
RETURNS BOOLEAN
LANGUAGE sql IMMUTABLE
SET search_path = pg_catalog, pg_temp
AS $$
    SELECT EXISTS (
        SELECT 1
        FROM unnest(scopes) AS scope
        WHERE msgq.canonical_address(address) = msgq.canonical_address(scope)
           OR msgq.canonical_address(address) LIKE msgq.canonical_address(scope) || '/%'
    )
$$;

-- pgcrypto may live in public or in an extensions schema; this wrapper is the one place that knows.
CREATE OR REPLACE FUNCTION msgq.token_digest(token TEXT)
RETURNS BYTEA
LANGUAGE sql IMMUTABLE
SET search_path = pg_catalog, pg_temp, public, extensions, msgq
AS $$
    SELECT digest(token, 'sha256')
$$;

-- The queue name for an address: a readable slug plus a digest, capped where pgmq caps table
-- names. The digest is the identity, because pgmq lowercases what it is given.
CREATE OR REPLACE FUNCTION msgq.queue_name(address TEXT)
RETURNS TEXT
LANGUAGE sql IMMUTABLE
SET search_path = pg_catalog, pg_temp, public, extensions, msgq
AS $$
    SELECT left(
        lower(rtrim(left(rtrim(regexp_replace(address, '[^A-Za-z0-9]', '_', 'g'), '_'), 30), '_'))
        || '_' || left(encode(digest(address, 'sha256'), 'hex'), 16),
        47
    )::text
$$;

-- Create the queue for an address if it is missing. Idempotent end to end: the advisory lock covers
-- the engine's own create, which does not lock its notification trigger.
CREATE OR REPLACE FUNCTION msgq.ensure_queue(address TEXT)
RETURNS TEXT
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog, pg_temp
AS $$
DECLARE
    addr TEXT := msgq.canonical_address(address);
    name TEXT := msgq.queue_name(msgq.canonical_address(address));
BEGIN
    PERFORM pg_catalog.pg_advisory_xact_lock(pg_catalog.hashtext('msgq.create:' || name));
    IF to_regclass('pgmq.q_' || name) IS NULL THEN
        PERFORM pgmq.create(name);
        EXECUTE pg_catalog.format(
            'CREATE INDEX IF NOT EXISTS %I ON pgmq.%I (enqueued_at)',
            'q_' || name || '_enq_idx', 'q_' || name);
        PERFORM pgmq.enable_notify_insert(name, 250);
    END IF;
    INSERT INTO msgq.address (address, queue_name) VALUES (addr, name)
        ON CONFLICT ON CONSTRAINT address_pkey DO NOTHING;
    RETURN name;
END;
$$;

-- Who a credential belongs to, or an error naming why it cannot be used. The three refusals are
-- deliberately distinct: a deployment that cannot tell "no token" from "wrong token" cannot be
-- operated.
CREATE OR REPLACE FUNCTION msgq.identity_of(token TEXT)
RETURNS msgq.identity
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog, pg_temp
AS $$
DECLARE
    ident msgq.identity;
BEGIN
    IF token IS NULL OR btrim(token) = '' THEN
        RAISE EXCEPTION 'msgq: no credential was presented; set MSGQ_TOKEN or write the identity file'
            USING ERRCODE = '28000';
    END IF;
    SELECT * INTO ident FROM msgq.identity WHERE digest = msgq.token_digest(token);
    IF ident.name IS NULL THEN
        RAISE EXCEPTION 'msgq: the credential is not recognised' USING ERRCODE = '28P01';
    END IF;
    IF ident.expires_at IS NOT NULL AND ident.expires_at <= now() THEN
        RAISE EXCEPTION 'msgq: the credential expired at %', ident.expires_at
            USING ERRCODE = '28P01';
    END IF;
    RETURN ident;
END;
$$;

-- Whether this bus has nobody to check against: no credential presented and no identity registered.
-- The read path mirrors the write path, so a single-user server stays usable; the first registered
-- identity closes the door for reads exactly as it already does for writes.
CREATE OR REPLACE FUNCTION msgq.open_bus(p_token TEXT)
RETURNS BOOLEAN
LANGUAGE sql
STABLE
SET search_path = pg_catalog, pg_temp
AS $$
    SELECT coalesce(btrim(p_token), '') = '' AND NOT EXISTS (SELECT 1 FROM msgq.identity)
$$;

-- Who the stored record says wrote it.
--
-- The identity is the default. A record may name an address inside the identity's own scopes,
-- which is how one credential serves several sessions. What the scope check buys is authorization,
-- not authorship: every session of one principal holds the same credential, so the bus cannot tell
-- them apart, and one of them can name another's address. Telling sessions apart needs a signature
-- the reader checks, which lives in the client and not here.
CREATE OR REPLACE FUNCTION msgq.sender_of(record JSONB, ident msgq.identity)
RETURNS TEXT
LANGUAGE plpgsql
IMMUTABLE
SET search_path = pg_catalog, pg_temp
AS $$
DECLARE
    claimed TEXT := msgq.canonical_address(coalesce(record ->> 'from', ''));
BEGIN
    IF claimed = '' THEN
        RAISE EXCEPTION 'msgq: the record names no sender' USING ERRCODE = '22023';
    END IF;
    IF NOT msgq.in_scope(claimed, ident.scopes) THEN
        RAISE EXCEPTION 'msgq: % is outside the scopes of %', claimed, ident.name
            USING ERRCODE = '42501';
    END IF;
    -- A room is not a sender: a participant speaks as itself, never as the place it speaks in.
    IF claimed !~ '^(session|cwd|cli)/[^/]' THEN
        RAISE EXCEPTION 'msgq: % is not a participant address', claimed USING ERRCODE = '22023';
    END IF;
    RETURN claimed;
END;
$$;

-- The only write path for a runtime role.
CREATE OR REPLACE FUNCTION msgq.publish_as(token TEXT, address TEXT, record JSONB)
RETURNS BIGINT
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog, pg_temp
AS $$
DECLARE
    ident msgq.identity := msgq.identity_of(token);
    addr TEXT := msgq.canonical_address(address);
    payload JSONB := record;
    sender TEXT;
    name TEXT;
    new_offset BIGINT;
BEGIN
    IF NOT msgq.in_scope(addr, ident.scopes) THEN
        RAISE EXCEPTION 'msgq: % is outside the scopes of %', addr, ident.name
            USING ERRCODE = '42501';
    END IF;

    -- A writer may speak as any address it is scoped for — a session per process, under one
    -- credential — but never as another user's, and never as something that is not an address.
    sender := msgq.sender_of(payload, ident);
    payload := jsonb_set(payload, '{from}', to_jsonb(sender), true);
    payload := jsonb_set(payload, '{to}', to_jsonb(addr), true);
    -- A credential has no business in a record, and neither has anything that resembles one.
    payload := payload - 'token' - 'credential' - 'secret';
    IF payload ? 'meta' THEN
        payload := jsonb_set(
            payload, '{meta}',
            (payload -> 'meta') - 'token' - 'credential' - 'secret');
    END IF;

    name := msgq.ensure_queue(addr);
    -- Ownership follows the first write: an address belongs to the principal that opened it, and
    -- prune authority follows ownership rather than read access.
    UPDATE msgq.address SET owner = ident.name
        WHERE msgq.address.address = addr AND msgq.address.owner = '';
    SELECT sent INTO new_offset
    FROM pgmq.send(
        name, payload,
        jsonb_build_object('msgq', jsonb_build_object('address', addr, 'identity', ident.name)))
        AS sent;
    UPDATE msgq.address SET newest_offset = GREATEST(msgq.address.newest_offset, new_offset)
        WHERE msgq.address.address = addr;
    RETURN new_offset;
END;
$$;

-- Mint an identity and return the credential once. Operators run this; the bus never does.
CREATE OR REPLACE FUNCTION msgq.register_identity(
    identity_name TEXT,
    scopes TEXT[],
    expires_at TIMESTAMPTZ DEFAULT NULL
)
RETURNS TEXT
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog, pg_temp, public, extensions, msgq
AS $$
DECLARE
    token TEXT := encode(gen_random_bytes(32), 'hex');
    canonical TEXT := msgq.canonical_address(identity_name);
BEGIN
    -- The identity is the sender the bus stamps, so it has to be an address a peer can answer:
    -- one of the reserved schemes and a non-empty name.
    IF position('/' IN canonical) > 0 THEN
        RAISE EXCEPTION 'msgq: an identity is a principal name, not an address: %',
            identity_name USING ERRCODE = '22023';
    END IF;
    INSERT INTO msgq.identity (name, digest, scopes, expires_at)
    VALUES (canonical, msgq.token_digest(token), scopes, expires_at)
    ON CONFLICT ON CONSTRAINT identity_pkey DO UPDATE
        SET digest = EXCLUDED.digest,
            scopes = EXCLUDED.scopes,
            expires_at = EXCLUDED.expires_at;
    RETURN token;
END;
$$;

-- What a runtime role may do: execute the write path, read the bus's own bookkeeping, and touch no
-- stream directly. Kept as a function so an operator and a test apply exactly the same rules.
CREATE OR REPLACE FUNCTION msgq.grant_runtime(role_name TEXT)
RETURNS void
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog, pg_temp
AS $$
BEGIN
    IF NOT EXISTS (SELECT 1 FROM pg_roles WHERE rolname = role_name) THEN
        RAISE EXCEPTION 'msgq: no role named %', role_name USING ERRCODE = '42704';
    END IF;

    EXECUTE format('GRANT USAGE ON SCHEMA msgq TO %I', role_name);
    EXECUTE format('GRANT SELECT ON msgq.meta TO %I', role_name);
    EXECUTE format(
        'GRANT EXECUTE ON FUNCTION msgq.publish_as(TEXT, TEXT, JSONB),'
        ' msgq.read_since(TEXT, TEXT, BIGINT, INT), msgq.stream_of(TEXT, TEXT),'
        ' msgq.catalog(TEXT, BOOLEAN) TO %I', role_name);

    -- Reads go through the functions too: the bookkeeping tables are not a side door.
    EXECUTE format(
        'REVOKE SELECT ON msgq.address, msgq.cursor, msgq.retention FROM %I', role_name);
    EXECUTE format(
        'REVOKE INSERT, UPDATE, DELETE ON msgq.cursor, msgq.retention FROM %I', role_name);
    EXECUTE format('GRANT EXECUTE ON FUNCTION msgq.identity_of(TEXT) TO %I', role_name);
    EXECUTE format('GRANT EXECUTE ON FUNCTION msgq.ensure_queue(TEXT) TO %I', role_name);

    -- Writes go through the function, so the streams themselves stay closed to this role.
    EXECUTE format('REVOKE CREATE ON SCHEMA pgmq FROM %I', role_name);
    EXECUTE format(
        'REVOKE INSERT, UPDATE, DELETE ON ALL TABLES IN SCHEMA pgmq FROM %I', role_name);
    EXECUTE format(
        'REVOKE ALL ON ALL FUNCTIONS IN SCHEMA pgmq FROM %I', role_name);
END;
$$;


DROP FUNCTION IF EXISTS msgq.read_since(TEXT, TEXT, BIGINT, INT);
DROP FUNCTION IF EXISTS msgq.prune(TEXT, TEXT, BIGINT, INT);

-- The read surface. Same shape as the write path: the credential is resolved first, the address second,
-- the table third. A caller holds no privilege on the streams, so these functions are the only door.
CREATE OR REPLACE FUNCTION msgq.read_since(
    p_token TEXT, p_address TEXT, p_since BIGINT DEFAULT 0, p_limit INT DEFAULT 100)
RETURNS TABLE (msg_id BIGINT, record JSONB, created_at BIGINT)
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog, pg_temp
AS $$
DECLARE
    ident msgq.identity;
    open_call BOOLEAN;
    addr TEXT := msgq.canonical_address(p_address);
    queue TEXT;
    rows INT := greatest(1, least(coalesce(p_limit, 100), 1000));
BEGIN
    open_call := msgq.open_bus(p_token);
    IF NOT open_call THEN
        SELECT * INTO ident FROM msgq.identity_of(p_token);
    END IF;
    IF NOT open_call AND NOT msgq.in_scope(addr, ident.scopes) THEN
        RAISE EXCEPTION 'msgq: % is outside the scopes of %', addr, ident.name
            USING ERRCODE = '42501';
    END IF;

    SELECT q.queue_name INTO queue FROM msgq.address q WHERE q.address = addr;
    IF queue IS NULL THEN
        RETURN;
    END IF;

    RETURN QUERY EXECUTE format(
        'SELECT s.msg_id, s.message, EXTRACT(EPOCH FROM s.enqueued_at)::bigint'
        ' FROM pgmq.%I s WHERE s.msg_id > $1 ORDER BY s.msg_id LIMIT $2',
        'q_' || queue) USING greatest(coalesce(p_since, 0), 0), rows;
END;
$$;

-- Where a stream starts, where it has got to, and how much has already gone: one call, because a
-- reader that asks three times can be told three different truths.
DROP FUNCTION IF EXISTS msgq.stream_of(TEXT, TEXT);

CREATE OR REPLACE FUNCTION msgq.stream_of(p_token TEXT, p_address TEXT)
RETURNS TABLE (head BIGINT, oldest BIGINT, dropped BIGINT, cut BIGINT)
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog, pg_temp
AS $$
DECLARE
    ident msgq.identity;
    open_call BOOLEAN;
    addr TEXT := msgq.canonical_address(p_address);
    queue TEXT;
BEGIN
    open_call := msgq.open_bus(p_token);
    IF NOT open_call THEN
        SELECT * INTO ident FROM msgq.identity_of(p_token);
    END IF;
    IF NOT open_call AND NOT msgq.in_scope(addr, ident.scopes) THEN
        RAISE EXCEPTION 'msgq: % is outside the scopes of %', addr, ident.name
            USING ERRCODE = '42501';
    END IF;

    SELECT q.queue_name, coalesce(q.newest_offset, 0) INTO queue, head
        FROM msgq.address q WHERE q.address = addr;
    IF queue IS NULL THEN
        head := 0;
        oldest := 0;
        dropped := 0;
        cut := 0;
        RETURN NEXT;
        RETURN;
    END IF;

    EXECUTE format('SELECT coalesce(min(s.msg_id), 0) FROM pgmq.%I s', 'q_' || queue) INTO oldest;
    SELECT coalesce(max(r.removed_total), 0), coalesce(max(r.cut_offset), 0)
      INTO dropped, cut FROM msgq.retention r WHERE r.topic = addr;
    RETURN NEXT;
END;
$$;

-- A session may stay silent for longer than its lease and then continue. Renewal extends the same row:
-- it never re-mints, because a new digest would log out every sibling session sharing the credential.
CREATE OR REPLACE FUNCTION msgq.renew_credential(p_token TEXT, p_lease INTERVAL DEFAULT '1 hour')
RETURNS TIMESTAMPTZ
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog, pg_temp
AS $$
DECLARE
    presented BYTEA := msgq.token_digest(p_token);
    extended TIMESTAMPTZ;
BEGIN
    IF p_token IS NULL OR btrim(p_token) = '' THEN
        RAISE EXCEPTION 'msgq: no credential was presented' USING ERRCODE = '28000';
    END IF;

    -- Deliberately not identity_of: a lease that has already elapsed is exactly the case being
    -- renewed, and refusing here would make a resumed session unrecoverable.
    UPDATE msgq.identity i
       SET expires_at = now() + coalesce(p_lease, '1 hour')
     WHERE i.digest = presented
    RETURNING i.expires_at INTO extended;

    IF extended IS NULL THEN
        RAISE EXCEPTION 'msgq: the credential is not recognised' USING ERRCODE = '28P01';
    END IF;
    RETURN extended;
END;
$$;

-- How much a particular consumer has lost: what was removed after the point it had reached. Exact at a
-- prune boundary, an over-report inside the last batch, which is the honest direction for a warning.
CREATE OR REPLACE FUNCTION msgq.dropped(p_token TEXT, p_address TEXT, p_cursor BIGINT)
RETURNS BIGINT
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog, pg_temp
AS $$
DECLARE
    ident msgq.identity;
    open_call BOOLEAN;
    addr TEXT := msgq.canonical_address(p_address);
    lost BIGINT;
BEGIN
    open_call := msgq.open_bus(p_token);
    IF NOT open_call THEN
        SELECT * INTO ident FROM msgq.identity_of(p_token);
    END IF;
    IF NOT open_call AND NOT msgq.in_scope(addr, ident.scopes) THEN
        RAISE EXCEPTION 'msgq: % is outside the scopes of %', addr, ident.name
            USING ERRCODE = '42501';
    END IF;

    SELECT coalesce(max(r.removed_total), 0)
         - coalesce((SELECT max(r.removed_total) FROM msgq.retention r
                      WHERE r.topic = addr AND r.cut_offset <= greatest(coalesce(p_cursor, 0), 0)), 0)
      INTO lost
      FROM msgq.retention r WHERE r.topic = addr;
    RETURN greatest(lost, 0);
END;
$$;

-- What this caller may hold: the addresses in its scopes and the cursors it owns.
DROP FUNCTION IF EXISTS msgq.catalog(TEXT);

CREATE OR REPLACE FUNCTION msgq.catalog(p_token TEXT, p_counts BOOLEAN DEFAULT TRUE)
RETURNS TABLE (kind TEXT, name TEXT, address TEXT, detail JSONB)
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog, pg_temp
AS $$
DECLARE
    ident msgq.identity;
    open_call BOOLEAN;
    row RECORD;
    total BIGINT;
BEGIN
    open_call := msgq.open_bus(p_token);
    IF NOT open_call THEN
        SELECT * INTO ident FROM msgq.identity_of(p_token);
    END IF;

    FOR row IN
        SELECT t.address, t.queue_name, coalesce(t.newest_offset, 0) AS head
          FROM msgq.address t
         WHERE open_call OR msgq.in_scope(t.address, ident.scopes)
         ORDER BY t.address
    LOOP
        -- Counting is what makes a listing cost grow with history, so a caller that prints no count
        -- is not made to pay for one.
        total := 0;
        IF p_counts THEN
            EXECUTE format('SELECT count(*) FROM pgmq.%I', 'q_' || row.queue_name) INTO total;
        END IF;
        kind := 'address';
        name := row.address;
        address := row.address;
        detail := jsonb_build_object(
            'head', row.head, 'count', total,
            'dropped', coalesce(
                (SELECT max(r.removed_total) FROM msgq.retention r WHERE r.topic = row.address), 0));
        RETURN NEXT;
    END LOOP;

    -- A cursor belongs to whoever wrote it: another principal's consumer name is not this caller's row.
    RETURN QUERY
        SELECT 'cursor'::TEXT, c.consumer, c.address,
               jsonb_build_object('offset', c.last_msg_id, 'owner', c.owner)
        FROM msgq.cursor c
        WHERE open_call OR (c.owner = ident.name AND msgq.in_scope(c.address, ident.scopes))
        ORDER BY c.address, c.consumer;
END;
$$;

-- A cursor belongs to the principal that wrote it, so a reused consumer name is a new position rather
-- than a moved one. The owner is stamped here and never passed in.
CREATE OR REPLACE FUNCTION msgq.cursor(
    p_token TEXT, p_address TEXT, p_consumer TEXT, p_offset BIGINT DEFAULT NULL)
RETURNS BIGINT
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog, pg_temp
AS $$
DECLARE
    ident msgq.identity;
    addr TEXT := msgq.canonical_address(p_address);
    name TEXT := coalesce(nullif(btrim(p_consumer), ''), 'default');
    position BIGINT;
BEGIN
    SELECT * INTO ident FROM msgq.identity_of(p_token);
    IF NOT msgq.in_scope(addr, ident.scopes) THEN
        RAISE EXCEPTION 'msgq: % is outside the scopes of %', addr, ident.name
            USING ERRCODE = '42501';
    END IF;

    IF p_offset IS NULL THEN
        SELECT c.last_msg_id INTO position
            FROM msgq.cursor c
            WHERE c.owner = ident.name AND c.consumer = name AND c.address = addr;
        RETURN coalesce(position, 0);
    END IF;

    INSERT INTO msgq.cursor (owner, consumer, address, last_msg_id)
    VALUES (ident.name, name, addr, greatest(p_offset, 0))
    ON CONFLICT (owner, consumer, address)
    DO UPDATE SET last_msg_id = greatest(msgq.cursor.last_msg_id, EXCLUDED.last_msg_id)
    RETURNING last_msg_id INTO position;
    RETURN position;
END;
$$;

CREATE OR REPLACE FUNCTION msgq.cursor_reset(p_token TEXT, p_address TEXT, p_consumer TEXT)
RETURNS BIGINT
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog, pg_temp
AS $$
DECLARE
    ident msgq.identity;
    addr TEXT := msgq.canonical_address(p_address);
    name TEXT := coalesce(nullif(btrim(p_consumer), ''), 'default');
    gone BIGINT;
BEGIN
    SELECT * INTO ident FROM msgq.identity_of(p_token);
    IF NOT msgq.in_scope(addr, ident.scopes) THEN
        RAISE EXCEPTION 'msgq: % is outside the scopes of %', addr, ident.name
            USING ERRCODE = '42501';
    END IF;
    DELETE FROM msgq.cursor c
        WHERE c.owner = ident.name AND c.consumer = name AND c.address = addr;
    GET DIAGNOSTICS gone = ROW_COUNT;
    RETURN gone;
END;
$$;

-- Prune is the only operation that removes records, and therefore the only one that may claim what was
-- removed. Authority follows ownership of the address: read access is not a licence to erase history.
DROP FUNCTION IF EXISTS msgq.prune(TEXT, TEXT, BIGINT, BIGINT[]);

CREATE OR REPLACE FUNCTION msgq.prune(
    p_token TEXT, p_address TEXT, p_before BIGINT,
    p_keep BIGINT[] DEFAULT ARRAY[]::BIGINT[], p_batch INT DEFAULT 500)
RETURNS BIGINT
LANGUAGE plpgsql
SECURITY DEFINER
SET search_path = pg_catalog, pg_temp
AS $$
DECLARE
    ident msgq.identity;
    addr TEXT := msgq.canonical_address(p_address);
    queue TEXT;
    owner TEXT;
    cut BIGINT := greatest(coalesce(p_before, 0), 0);
    keep_ids BIGINT[] := coalesce(p_keep, ARRAY[]::BIGINT[]);
    batch INT := greatest(1, least(coalesce(p_batch, 500), 10000));
    removed BIGINT;
    total BIGINT := 0;
BEGIN
    SELECT * INTO ident FROM msgq.identity_of(p_token);
    IF NOT msgq.in_scope(addr, ident.scopes) THEN
        RAISE EXCEPTION 'msgq: % is outside the scopes of %', addr, ident.name
            USING ERRCODE = '42501';
    END IF;

    SELECT a.queue_name, a.owner INTO queue, owner FROM msgq.address a WHERE a.address = addr;
    IF queue IS NULL THEN
        RETURN 0;
    END IF;
    IF owner IS DISTINCT FROM ident.name THEN
        RAISE EXCEPTION 'msgq: % belongs to %, not %', addr, coalesce(nullif(owner, ''), 'nobody'),
            ident.name USING ERRCODE = '42501';
    END IF;

    LOOP
        EXECUTE format(
            'DELETE FROM pgmq.%I WHERE ctid IN ('
            '  SELECT ctid FROM pgmq.%I WHERE msg_id <= $1 AND NOT (msg_id = ANY($3)) LIMIT $2)',
            'q_' || queue, 'q_' || queue) USING cut, batch, keep_ids;
        GET DIAGNOSTICS removed = ROW_COUNT;
        total := total + removed;
        EXIT WHEN removed < batch;
    END LOOP;

    IF total > 0 THEN
        INSERT INTO msgq.retention (topic, cut_offset, removed_total)
        VALUES (addr, cut, (SELECT coalesce(max(r.removed_total), 0) FROM msgq.retention r WHERE r.topic = addr) + total);
    END IF;
    RETURN total;
END;
$$;


-- cursor and prune are defined below grant_runtime, so their execute grants live here.
DO $$
BEGIN
    IF EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'msgq_runtime') THEN
        GRANT EXECUTE ON FUNCTION msgq.cursor(TEXT, TEXT, TEXT, BIGINT) TO msgq_runtime;
        GRANT EXECUTE ON FUNCTION msgq.prune(TEXT, TEXT, BIGINT, BIGINT[], INT) TO msgq_runtime;
        GRANT EXECUTE ON FUNCTION msgq.cursor_reset(TEXT, TEXT, TEXT) TO msgq_runtime;
        GRANT EXECUTE ON FUNCTION msgq.dropped(TEXT, TEXT, BIGINT) TO msgq_runtime;
        GRANT EXECUTE ON FUNCTION msgq.open_bus(TEXT) TO msgq_runtime;
        GRANT EXECUTE ON FUNCTION msgq.renew_credential(TEXT, INTERVAL) TO msgq_runtime;
    END IF;
END;
$$;

-- The grants run last: every function they name exists by this point.
DO $$
BEGIN
    IF EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'msgq_runtime') THEN
        PERFORM msgq.grant_runtime('msgq_runtime');
    END IF;
END;
$$;
