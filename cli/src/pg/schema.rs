//! The bus's own schema, and the small queries every method needs.

use postgres::Client;

use crate::error::Error;

/// The authorizing write path, kept as SQL so a deployment can review it and a runtime role can be
/// granted it without the bus's own code being in the trust boundary.
const AUTHORIZATION: &str = include_str!("../../../sql/authorization.sql");

/// The schema this build installs. Bump when `BOOTSTRAP` or the authorizing SQL changes: a bus whose
/// `msgq.meta` records this value is left alone, which is what keeps a command from rewriting eighteen
/// function bodies and 30 kB of WAL every time it runs.
pub const SCHEMA_VERSION: i32 = 2;

/// Everything the bus adds to a database. A fresh connection runs exactly this and touches no
/// pgmq table: creating a queue is a decision made per address, not at connect time.
pub const BOOTSTRAP: &str = "CREATE SCHEMA IF NOT EXISTS msgq;
                 CREATE TABLE IF NOT EXISTS msgq.meta (
                     key TEXT PRIMARY KEY,
                     value TEXT NOT NULL
                 );
                 CREATE TABLE IF NOT EXISTS msgq.address (
                     address TEXT PRIMARY KEY,
                     queue_name TEXT UNIQUE NOT NULL,
                     newest_offset BIGINT NOT NULL DEFAULT 0,
                     created_at TIMESTAMPTZ NOT NULL DEFAULT now()
                 );
                 ALTER TABLE msgq.address
                     ADD COLUMN IF NOT EXISTS newest_offset BIGINT NOT NULL DEFAULT 0;
                 CREATE TABLE IF NOT EXISTS msgq.cursor (
                     consumer TEXT NOT NULL,
                     address TEXT NOT NULL,
                     last_msg_id BIGINT NOT NULL,
                     PRIMARY KEY (consumer, address)
                 );
                 CREATE TABLE IF NOT EXISTS msgq.identity (
                     name TEXT PRIMARY KEY,
                     digest BYTEA NOT NULL UNIQUE,
                     scopes TEXT[] NOT NULL,
                     expires_at TIMESTAMPTZ,
                     created_at TIMESTAMPTZ NOT NULL DEFAULT now()
                 );
                 CREATE TABLE IF NOT EXISTS msgq.retention (
                     topic TEXT NOT NULL,
                     cut_offset BIGINT NOT NULL,
                     removed_total BIGINT NOT NULL,
                     removed_at TIMESTAMPTZ NOT NULL DEFAULT now(),
                     PRIMARY KEY (topic, cut_offset)
                 );";

/// Connect, install the bus's tables, and record what this build is.
///
/// Never creates a queue: a fresh connection touches nothing in the pgmq schema.
pub fn connect(config: postgres::Config) -> Result<Client, Error> {
    let mut client = config
        .connect(postgres::NoTls)
        .map_err(|e| Error::runtime(format!("cannot connect to the bus: {e}")))?;
    let installed: Option<i32> = client
        .query_opt(
            "SELECT value::int FROM msgq.meta WHERE key = 'schema_version'",
            &[],
        )
        .ok()
        .flatten()
        .map(|row| row.get(0));

    match install_decision(installed)? {
        false => {}
        true => {
            client
                .batch_execute(&format!("{BOOTSTRAP}\n{AUTHORIZATION}"))
                .map_err(|e| {
                    Error::runtime(format!("cannot bootstrap the bus schema: {}", pg(e)))
                })?;
            client
                .execute(
                    "INSERT INTO msgq.meta (key, value) VALUES ('schema_version', $1)
                     ON CONFLICT (key) DO UPDATE SET value = EXCLUDED.value",
                    &[&SCHEMA_VERSION.to_string()],
                )
                .map_err(|e| Error::runtime(format!("cannot record the schema version: {e}")))?;
        }
    }

    let created_by = format!("msgq {}", crate::version());
    let engine = pgmq_version(&mut client);
    for (key, value) in [
        ("protocol_version", crate::version()),
        ("created_by", created_by.as_str()),
        ("pgmq_version", engine.as_str()),
    ] {
        client
            .execute(
                "INSERT INTO msgq.meta (key, value) VALUES ($1, $2) ON CONFLICT (key) DO NOTHING",
                &[&key, &value],
            )
            .map_err(|e| Error::runtime(format!("cannot record {key}: {e}")))?;
    }
    Ok(client)
}

/// Whether this connection must install the schema, given what the bus records.
///
/// Separated from the connection so the decision is testable without a server: equal means leave the
/// bus alone, absent or older means install, newer means the client is the stale one.
fn install_decision(installed: Option<i32>) -> Result<bool, Error> {
    match installed {
        Some(version) if version == SCHEMA_VERSION => Ok(false),
        Some(version) if version > SCHEMA_VERSION => Err(Error::runtime(format!(
            "the bus holds schema {version} and this client installs {SCHEMA_VERSION}; upgrade the client"
        ))),
        _ => Ok(true),
    }
}

/// The installed pgmq version, or how it was installed when it is not an extension.
pub fn pgmq_version(client: &mut Client) -> String {
    let extension: Option<String> = client
        .query_opt(
            "SELECT extversion::text FROM pg_extension WHERE extname = 'pgmq'",
            &[],
        )
        .ok()
        .flatten()
        .map(|row| row.get(0));
    if let Some(version) = extension {
        return version;
    }
    match client.query_opt("SELECT to_regclass('pgmq.meta')::text", &[]) {
        Ok(Some(row)) => match row.get::<_, Option<String>>(0) {
            Some(_) => "sql-only".to_string(),
            None => "absent".to_string(),
        },
        _ => "unknown".to_string(),
    }
}

/// Map a driver error onto the bus's runtime error.
pub fn pg(e: postgres::Error) -> Error {
    // The driver's own Display is the unhelpful "db error"; the server says what happened.
    match e.as_db_error() {
        Some(db) => Error::runtime(format!(
            "shared engine: {} (SQLSTATE {}, {}:{})",
            db.message(),
            db.code().code(),
            db.table().unwrap_or("-"),
            db.column().unwrap_or("-")
        )),
        None => Error::runtime(format!("shared engine: {e}")),
    }
}

/// The queue name for an address, or nothing when the address was never published to.
pub fn queue_of(client: &mut Client, address: &str) -> Result<Option<String>, Error> {
    let row = client
        .query_opt(
            "SELECT queue_name FROM msgq.address WHERE address = $1",
            &[&address],
        )
        .map_err(pg)?;
    Ok(row.map(|row| row.get(0)))
}

/// A one-row count or aggregate, zero when nothing matched.
pub fn scalar_i64(
    client: &mut Client,
    sql: &str,
    params: &[&(dyn postgres::types::ToSql + Sync)],
) -> Result<i64, Error> {
    let row = client.query_one(sql, params).map_err(pg)?;
    Ok(row.get::<_, Option<i64>>(0).unwrap_or(0))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_schema_is_installed_only_when_it_is_owed() {
        assert!(
            install_decision(None).unwrap(),
            "a bus with no version needs one"
        );
        assert!(
            install_decision(Some(1)).unwrap(),
            "an older bus is upgraded"
        );
        assert!(
            !install_decision(Some(SCHEMA_VERSION)).unwrap(),
            "a current bus is left alone"
        );
    }

    #[test]
    fn a_newer_bus_is_refused() {
        let error = install_decision(Some(SCHEMA_VERSION + 1)).unwrap_err();
        let message = error.to_string();
        assert!(
            message.contains(&(SCHEMA_VERSION + 1).to_string()),
            "{message}"
        );
        assert!(message.contains(&SCHEMA_VERSION.to_string()), "{message}");
    }
}
