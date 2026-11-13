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

-- Who the stored record says wrote it.
--
-- The identity is the default. A record may name an address inside the identity's own scopes,
-- which is how one credential serves several sessions: each process speaks as itself, and none of
-- them can speak as somebody else.
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
    EXECUTE format(
        'GRANT SELECT ON msgq.address, msgq.cursor, msgq.retention, msgq.meta TO %I', role_name);
    EXECUTE format('GRANT INSERT, UPDATE, DELETE ON msgq.cursor, msgq.retention TO %I', role_name);
    EXECUTE format('GRANT EXECUTE ON FUNCTION msgq.publish_as(TEXT, TEXT, JSONB) TO %I', role_name);
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

DO $$
BEGIN
    IF EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'msgq_runtime') THEN
        PERFORM msgq.grant_runtime('msgq_runtime');
    END IF;
END;
$$;
