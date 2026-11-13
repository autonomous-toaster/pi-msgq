//! The bus over the shared engine: pgmq on PostgreSQL.
//!
//! pgmq owns storage, message ids and the queue lifecycle; the bus owns what a queue cannot express
//! — one stream per address with its own cursor, and retention watermarks. See design.md decisions
//! 2 to 7 for why the delivery path never calls `pgmq.read`.
//!
//! Every address maps to exactly one queue. pgmq's table names are lowercased and capped at 47
//! characters, so the name is derived (a readable slug plus a digest) and the true address is kept
//! twice: in `msgq.address` and in each record's `headers`.

use std::sync::{Mutex, MutexGuard};

mod naming;
mod schema;
mod wake;

pub use naming::queue_name;
use naming::{escaped, quote_ident};
use schema::{pg, queue_of, scalar_i64};

use postgres::Client;
use serde_json::Value;

use crate::error::Error;
use crate::store::{Bus, FollowLock, Removal, StoredRecord, StreamSummary, Wake};

/// Records removed per statement by a prune.
const PRUNE_BATCH_DEFAULT: usize = 500;

/// The bus over a PostgreSQL database with the pgmq schema installed.
pub struct PgStore {
    client: Mutex<Client>,
    /// Kept so the wake source can open its own connection: LISTEN needs a connection that is idle.
    config: postgres::Config,
    /// The credential this bus presents, if the deployment names one.
    credential: Option<String>,
}

impl PgStore {
    /// Connect and bootstrap the bus's own tables.
    ///
    /// The location is either a URI or libpq keyword settings; `MSGQ_PG*` variables win over
    /// whatever it said, so a URI can name the server while the secret stays in the environment.
    pub fn connect(dsn: &str, credential: Option<String>) -> Result<Self, Error> {
        let mut config: postgres::Config = dsn
            .parse()
            .map_err(|e| Error::usage(format!("unusable bus connection settings: {e}")))?;
        crate::config::apply_pg_env(&mut config)?;
        Ok(PgStore {
            client: Mutex::new(schema::connect(config.clone())?),
            config,
            credential,
        })
    }

    /// Write a record with no identity: the posture of a bus that has none registered.
    fn publish_direct(client: &mut Client, to: &str, payload: &str) -> Result<i64, Error> {
        let name: String = client
            .query_one("SELECT msgq.ensure_queue($1)", &[&to])
            .map_err(pg)?
            .get(0);
        let headers = serde_json::json!({ "msgq": { "address": to } });
        let row = client
            .query_one(
                &format!(
                    "SELECT pgmq.send('{}', $1::text::jsonb, $2::text::jsonb)",
                    escaped(&name)
                ),
                &[&payload, &headers.to_string()],
            )
            .map_err(pg)?;
        let offset: i64 = row.get(0);
        client
            .execute(
                "UPDATE msgq.address SET newest_offset = GREATEST(newest_offset, $1)
                 WHERE address = $2",
                &[&offset, &to],
            )
            .map_err(pg)?;
        Ok(offset)
    }

    /// The connection, locked for the length of one call.
    /// The credential this bus presents to the function surface. Every read and every cursor write
    /// resolves it, so a store without one cannot reach a tenant's stream at all.
    fn token(&self) -> Result<&str, Error> {
        self.credential.as_deref().ok_or_else(|| {
            Error::Runtime("the shared engine needs a credential to name the caller".into())
        })
    }

    fn conn(&self) -> Result<MutexGuard<'_, Client>, Error> {
        self.client
            .lock()
            .map_err(|_| Error::runtime("the shared bus connection was poisoned"))
    }
}

impl Bus for PgStore {
    fn engine(&self) -> &'static str {
        "shared"
    }

    fn ensure_address(&self, topic: &str) -> Result<(), Error> {
        let mut client = self.conn()?;
        client
            .execute("SELECT msgq.ensure_queue($1)", &[&topic])
            .map_err(pg)?;
        Ok(())
    }

    fn publish(&self, to: &str, record: &Value) -> Result<i64, Error> {
        let payload = serde_json::to_string(record).map_err(|e| Error::runtime(e.to_string()))?;
        let mut client = self.conn()?;
        // A bus with identities enforces them. A bus with none cannot authorize anything, so it
        // writes unauthenticated and says so in `db info` rather than pretending otherwise.
        let registered = scalar_i64(&mut client, "SELECT COUNT(*) FROM msgq.identity", &[])?;
        match (&self.credential, registered) {
            (Some(token), _) => {
                let row = client
                    .query_one(
                        "SELECT msgq.publish_as($1, $2, $3::text::jsonb)",
                        &[token, &to.to_string(), &payload],
                    )
                    .map_err(pg)?;
                Ok(row.get(0))
            }
            (None, 0) => PgStore::publish_direct(&mut client, to, &payload),
            (None, _) => Err(Error::runtime(
                "msgq: no credential was presented; set MSGQ_TOKEN or write ~/.config/msgq/token",
            )),
        }
    }

    fn identity(&self) -> Result<Option<(String, Vec<String>)>, Error> {
        let Some(token) = &self.credential else {
            return Ok(None);
        };
        let mut client = self.conn()?;
        let row = client
            .query_opt(
                "SELECT name, scopes FROM msgq.identity WHERE digest = msgq.token_digest($1)",
                &[token],
            )
            .map_err(pg)?;
        Ok(row.map(|row| (row.get(0), row.get::<_, Vec<String>>(1))))
    }

    fn credential(&self) -> Result<String, Error> {
        let mut client = self.conn()?;
        let registered = scalar_i64(&mut client, "SELECT COUNT(*) FROM msgq.identity", &[])?;
        match &self.credential {
            None if registered == 0 => {
                Ok("none (no identity is registered, so this bus cannot authorize)".to_string())
            }
            None => Ok(format!(
                "missing (this bus has {registered} identities and refuses writes without one)"
            )),
            Some(token) => {
                let row = client
                    .query_opt(
                        "SELECT name FROM msgq.identity WHERE digest = msgq.token_digest($1)",
                        &[token],
                    )
                    .map_err(pg)?;
                match row {
                    Some(row) => Ok(format!("accepted as {}", row.get::<_, String>(0))),
                    None => Ok("rejected (this bus does not know the credential)".to_string()),
                }
            }
        }
    }

    fn read_since(&self, topic: &str, offset: i64, limit: i64) -> Result<Vec<StoredRecord>, Error> {
        let mut client = self.conn()?;
        let Some(name) = queue_of(&mut client, topic)? else {
            return Ok(Vec::new());
        };
        let rows = client
            .query(
                &format!(
                    "SELECT msg_id, message::text, EXTRACT(EPOCH FROM enqueued_at)::bigint
                     FROM pgmq.{} WHERE msg_id > $1 ORDER BY msg_id LIMIT $2",
                    quote_ident(&format!("q_{name}"))
                ),
                &[&offset, &limit],
            )
            .map_err(pg)?;
        Ok(rows
            .into_iter()
            .map(|row| StoredRecord {
                topic: topic.to_string(),
                offset: row.get(0),
                payload: row.get::<_, String>(1).into_bytes(),
                created_at: row.get(2),
            })
            .collect())
    }

    fn head(&self, topic: &str) -> Result<i64, Error> {
        let mut client = self.conn()?;
        let Some(name) = queue_of(&mut client, topic)? else {
            return Ok(0);
        };
        scalar_i64(
            &mut client,
            &format!(
                "SELECT COALESCE(MAX(msg_id), 0) FROM pgmq.{}",
                quote_ident(&format!("q_{name}"))
            ),
            &[],
        )
    }

    fn cursor_get(&self, consumer: &str, topic: &str) -> Result<Option<i64>, Error> {
        let mut client = self.conn()?;
        let position = scalar_i64(
            &mut client,
            "SELECT msgq.cursor($1, $2, $3, NULL)",
            &[&self.token()?, &topic, &consumer],
        )?;
        // Zero is both "never written" and "at the start", and a reader treats them the same way.
        Ok((position != 0).then_some(position))
    }

    fn cursor_set(&self, consumer: &str, topic: &str, offset: i64) -> Result<bool, Error> {
        let mut client = self.conn()?;
        let stored = scalar_i64(
            &mut client,
            "SELECT msgq.cursor($1, $2, $3, $4)",
            &[&self.token()?, &topic, &consumer, &offset],
        )?;
        Ok(stored == offset)
    }

    fn cursor_reset(&self, consumer: &str, topic: &str) -> Result<usize, Error> {
        let mut client = self.conn()?;
        let gone = scalar_i64(
            &mut client,
            "SELECT msgq.cursor_reset($1, $2, $3)",
            &[&self.token()?, &topic, &consumer],
        )?;
        Ok(gone as usize)
    }

    fn prune_before(
        &self,
        topic: &str,
        cut: i64,
        keep: &[i64],
        batch: usize,
    ) -> Result<Removal, Error> {
        let mut client = self.conn()?;
        // The function removes in batches and writes the watermark itself, because the watermark is a
        // claim about what was removed and only the operation that removes it may make that claim.
        let size = batch.max(1) as i64;
        let removed = scalar_i64(
            &mut client,
            "SELECT msgq.prune($1, $2, $3, $4::bigint[], $5::bigint::int)",
            &[&self.token()?, &topic, &cut, &keep.to_vec(), &size],
        )?;
        Ok(Removal {
            removed: removed as usize,
            batches: (removed / size + i64::from(removed % size > 0)) as usize,
        })
    }

    fn list_addresses(&self) -> Result<Vec<(String, i64, i64)>, Error> {
        let mut client = self.conn()?;
        let rows = client
            .query(
                "SELECT name, (detail ->> 'count')::bigint, (detail ->> 'head')::bigint
                 FROM msgq.catalog($1) WHERE kind = 'address' ORDER BY name",
                &[&self.token()?],
            )
            .map_err(pg)?;
        Ok(rows
            .into_iter()
            .map(|row| (row.get(0), row.get(1), row.get(2)))
            .collect())
    }

    fn list_consumers(&self, topic: Option<&str>) -> Result<Vec<(String, String, i64)>, Error> {
        let mut client = self.conn()?;
        let rows = match topic {
            Some(topic) => client
                .query(
                    "SELECT name, address, (detail ->> 'offset')::bigint FROM msgq.catalog($1)
                     WHERE kind = 'cursor' AND address = $2 ORDER BY name",
                    &[&self.token()?, &topic],
                )
                .map_err(pg)?,
            None => client
                .query(
                    "SELECT name, address, (detail ->> 'offset')::bigint FROM msgq.catalog($1)
                     WHERE kind = 'cursor' ORDER BY address, name",
                    &[&self.token()?],
                )
                .map_err(pg)?,
        };
        Ok(rows
            .into_iter()
            .map(|row| (row.get(0), row.get(1), row.get(2)))
            .collect())
    }

    fn delete_consumers(&self, name: &str, topic: Option<&str>) -> Result<usize, Error> {
        let mut client = self.conn()?;
        let addresses: Vec<String> = match topic {
            Some(topic) => vec![topic.to_string()],
            None => client
                .query(
                    "SELECT address FROM msgq.catalog($1) WHERE kind = 'cursor' AND name = $2",
                    &[&self.token()?, &name],
                )
                .map_err(pg)?
                .into_iter()
                .map(|row| row.get(0))
                .collect(),
        };
        let mut gone = 0usize;
        for address in addresses {
            gone += scalar_i64(
                &mut client,
                "SELECT msgq.cursor_reset($1, $2, $3)",
                &[&self.token()?, &address, &name],
            )? as usize;
        }
        Ok(gone)
    }

    fn wake(&self, topics: &[&str]) -> Box<dyn Wake> {
        // Polling is the guaranteed path. A listening connection is an optimisation: a pooler in
        // transaction mode cannot keep a LISTEN alive, and a bus that depends on an optimisation
        // stalls whenever the deployment changes shape.
        wake::source(self.config.clone(), topics)
    }

    fn try_lock(
        &self,
        name: &str,
        _owner: &str,
        _ttl_s: i64,
    ) -> Result<Option<Box<dyn FollowLock>>, Error> {
        let mut client = self.conn()?;
        let held: bool = client
            .query_one("SELECT pg_try_advisory_lock(hashtext($1))", &[&name])
            .map_err(pg)?
            .get(0);
        if held {
            Ok(Some(Box::new(NoopLock)))
        } else {
            Ok(None)
        }
    }

    fn meta(&self, key: &str) -> Result<Option<String>, Error> {
        let mut client = self.conn()?;
        let row = client
            .query_opt("SELECT value FROM msgq.meta WHERE key = $1", &[&key])
            .map_err(pg)?;
        Ok(row.map(|row| row.get(0)))
    }

    fn compact(&self) -> Result<(), Error> {
        // Vacuum cannot run inside a transaction, and the engine's own autovacuum owns this.
        Ok(())
    }

    fn stream_summaries(&self) -> Result<Vec<StreamSummary>, Error> {
        let mut out = Vec::new();
        for (address, records, newest) in self.list_addresses()? {
            let oldest: i64 = {
                let mut client = self.conn()?;
                let Some(name) = queue_of(&mut client, &address)? else {
                    continue;
                };
                let row = client
                    .query_one(
                        &format!(
                            "SELECT COALESCE(EXTRACT(EPOCH FROM MIN(enqueued_at))::bigint, 0)
                             FROM pgmq.{}",
                            quote_ident(&format!("q_{name}"))
                        ),
                        &[],
                    )
                    .map_err(pg)?;
                row.get(0)
            };
            out.push(StreamSummary {
                topic: address,
                records,
                oldest_at: oldest,
                newest_offset: newest,
            });
        }
        Ok(out)
    }

    fn prefix_end(&self, topic: &str, threshold: i64) -> Result<Option<i64>, Error> {
        let mut client = self.conn()?;
        let Some(name) = queue_of(&mut client, topic)? else {
            return Ok(None);
        };
        let row = client
            .query_one(
                &format!(
                    "SELECT MAX(msg_id) FROM pgmq.{} WHERE enqueued_at < to_timestamp($1)",
                    quote_ident(&format!("q_{name}"))
                ),
                &[&(threshold as f64)],
            )
            .map_err(pg)?;
        Ok(row.get::<_, Option<i64>>(0))
    }

    fn count_stream_prefix(&self, topic: &str, cut: i64, keep: &[i64]) -> Result<i64, Error> {
        let mut client = self.conn()?;
        let Some(name) = queue_of(&mut client, topic)? else {
            return Ok(0);
        };
        scalar_i64(
            &mut client,
            &format!(
                "SELECT COUNT(*) FROM pgmq.{} WHERE msg_id <= $1 AND NOT (msg_id = ANY($2))",
                quote_ident(&format!("q_{name}"))
            ),
            &[&cut, &keep],
        )
    }

    fn newest_presence_offsets(&self, topic: &str) -> Result<Vec<i64>, Error> {
        let mut client = self.conn()?;
        let Some(name) = queue_of(&mut client, topic)? else {
            return Ok(Vec::new());
        };
        let rows = client
            .query(
                &format!(
                    "SELECT MAX(msg_id) FROM pgmq.{}
                     GROUP BY COALESCE(message->'event'->>'participant', message->>'from')",
                    quote_ident(&format!("q_{name}"))
                ),
                &[],
            )
            .map_err(pg)?;
        Ok(rows.into_iter().map(|row| row.get(0)).collect())
    }

    fn oldest_offset(&self, topic: &str) -> Result<Option<i64>, Error> {
        let mut client = self.conn()?;
        let Some(name) = queue_of(&mut client, topic)? else {
            return Ok(None);
        };
        let row = client
            .query_one(
                &format!(
                    "SELECT MIN(msg_id) FROM pgmq.{}",
                    quote_ident(&format!("q_{name}"))
                ),
                &[],
            )
            .map_err(pg)?;
        Ok(row.get::<_, Option<i64>>(0))
    }

    fn retention_record(&self, topic: &str, cut: i64, removed_total: i64) -> Result<(), Error> {
        // msgq.prune wrote the watermark as part of removing the records. A second writer would be a
        // claim about a removal that did not happen here.
        let _ = (topic, cut, removed_total);
        Ok(())
    }

    fn retention_watermark(&self, topic: &str) -> Result<Option<(i64, i64)>, Error> {
        let mut client = self.conn()?;
        let row = client
            .query_one(
                "SELECT cut, dropped FROM msgq.stream_of($1, $2)",
                &[&self.token()?, &topic],
            )
            .map_err(pg)?;
        let cut: i64 = row.get(0);
        let removed: i64 = row.get(1);
        Ok((cut > 0).then_some((cut, removed)))
    }

    fn retention_dropped(&self, topic: &str, cursor: i64) -> Result<i64, Error> {
        let mut client = self.conn()?;
        // What was removed after the point this consumer had reached.
        scalar_i64(
            &mut client,
            "SELECT msgq.dropped($1, $2, $3)",
            &[&self.token()?, &topic, &cursor],
        )
    }

    fn record_at(&self, _offset: i64) -> Result<Option<(String, String)>, Error> {
        // A pgmq id is per queue, so an offset alone names no record. Callers hold the address.
        Ok(None)
    }
}

/// A follow lock held by the connection's advisory lock; nothing to renew.
struct NoopLock;

impl FollowLock for NoopLock {
    fn heartbeat(&self, _ttl_s: i64) -> Result<(), Error> {
        Ok(())
    }
}

/// The default batch size a caller may pass when it has no opinion.
pub const fn default_batch() -> usize {
    PRUNE_BATCH_DEFAULT
}
