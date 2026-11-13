//! Storage layer: a thin wrapper over honker's durable streams.
//!
//! All engine use is confined to this module so the record contract and
//! address grammar stay independent of the engine.

use std::path::{Path, PathBuf};

use honker::{Database, Lock, UpdateEvents};

use crate::error::Error;
use crate::record::PROTOCOL_VERSION;

/// One stored record, in the shape the bus promises rather than the shape the engine uses.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredRecord {
    /// The address whose stream holds the record.
    pub topic: String,
    /// The offset assigned within that stream.
    pub offset: i64,
    /// When the bus stored the record, in unix seconds. Liveness is judged by this, never by a
    /// clock the publisher claims.
    pub created_at: i64,
    /// The stored record, as JSON bytes.
    pub payload: Vec<u8>,
}

/// A waiter's wake source: blocks until the bus may have new records, or the slice elapses.
pub trait Wake: Send {
    fn wait(&self, slice: std::time::Duration) -> Result<(), Error>;
}

/// A held follow lock, renewed while the follow runs.
pub trait FollowLock: Send {
    fn heartbeat(&self, ttl_s: i64) -> Result<(), Error>;
}

/// The storage contract behind the bus.
///
/// Every method here is engine-neutral: the CLI above this boundary sees records, offsets and
/// cursors, never a table or a driver type. Two implementations exist by design (design.md,
/// decision 1) and both must satisfy the behavioural fixtures.
pub trait Bus: Send {
    /// Which engine serves this bus, for reporting.
    fn engine(&self) -> &'static str;
    fn ensure_address(&self, topic: &str) -> Result<(), Error>;
    fn publish(&self, to: &str, record: &serde_json::Value) -> Result<i64, Error>;
    fn read_since(&self, topic: &str, offset: i64, limit: i64) -> Result<Vec<StoredRecord>, Error>;
    fn head(&self, topic: &str) -> Result<i64, Error>;
    fn cursor_get(&self, consumer: &str, topic: &str) -> Result<Option<i64>, Error>;
    fn cursor_set(&self, consumer: &str, topic: &str, offset: i64) -> Result<bool, Error>;
    fn cursor_reset(&self, consumer: &str, topic: &str) -> Result<usize, Error>;
    fn prune_before(
        &self,
        topic: &str,
        cut: i64,
        keep: &[i64],
        batch: usize,
    ) -> Result<Removal, Error>;
    fn list_addresses(&self) -> Result<Vec<(String, i64, i64)>, Error>;
    fn list_consumers(&self, topic: Option<&str>) -> Result<Vec<(String, String, i64)>, Error>;
    fn delete_consumers(&self, name: &str, topic: Option<&str>) -> Result<usize, Error>;
    /// A wake source for the named topics. An engine that cannot subscribe falls back to polling.
    fn wake(&self, topics: &[&str]) -> Box<dyn Wake>;
    fn try_lock(
        &self,
        name: &str,
        owner: &str,
        ttl_s: i64,
    ) -> Result<Option<Box<dyn FollowLock>>, Error>;
    fn meta(&self, key: &str) -> Result<Option<String>, Error>;
    fn compact(&self) -> Result<(), Error>;
    fn stream_summaries(&self) -> Result<Vec<StreamSummary>, Error>;
    fn prefix_end(&self, topic: &str, threshold: i64) -> Result<Option<i64>, Error>;
    fn count_stream_prefix(&self, topic: &str, cut: i64, keep: &[i64]) -> Result<i64, Error>;
    fn newest_presence_offsets(&self, topic: &str) -> Result<Vec<i64>, Error>;
    fn oldest_offset(&self, topic: &str) -> Result<Option<i64>, Error>;
    fn retention_record(&self, topic: &str, cut: i64, removed_total: i64) -> Result<(), Error>;
    fn retention_watermark(&self, topic: &str) -> Result<Option<(i64, i64)>, Error>;
    fn retention_dropped(&self, topic: &str, cursor: i64) -> Result<i64, Error>;
    fn record_at(&self, offset: i64) -> Result<Option<(String, String)>, Error>;
}

/// One stream's size, and how old its oldest record is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StreamSummary {
    pub topic: String,
    pub records: i64,
    /// When the oldest record still stored in this stream was written.
    pub oldest_at: i64,
    pub newest_offset: i64,
}

/// How much a removal took, and how many statements it took to take it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Removal {
    pub removed: usize,
    pub batches: usize,
}

/// An open message bus.
pub struct LocalStore {
    db: Database,
}

impl LocalStore {
    /// Open (creating if needed) the bus and ensure owned metadata exists.
    ///
    /// Returns the store and whether the database file was created here.
    pub fn open(path: &Path) -> Result<(Self, bool), Error> {
        let created = !path.exists();

        if let Some(parent) = path.parent()
            && !parent.as_os_str().is_empty()
        {
            std::fs::create_dir_all(parent)
                .map_err(|e| Error::runtime(format!("cannot create {}: {e}", parent.display())))?;
        }

        let db = Database::open(path)
            .map_err(|e| Error::runtime(format!("cannot open {}: {e}", path.display())))?;
        let store = LocalStore { db };
        store.init_meta()?;

        if created {
            store.restrict_permissions(path)?;
        }
        Ok((store, created))
    }

    fn init_meta(&self) -> Result<(), Error> {
        let version = PROTOCOL_VERSION.to_string();
        let created_by = format!("msgq {}", crate::version());
        self.db
            .with_conn(|c| -> Result<(), String> {
                c.execute_batch(
                    "CREATE TABLE IF NOT EXISTS msgq_meta (                     key TEXT PRIMARY KEY, value TEXT NOT NULL)",
                )
                .map_err(|e| e.to_string())?;
                c.execute(
                    "INSERT OR IGNORE INTO msgq_meta (key, value) VALUES (?1, ?2)",
                    ["protocol_version", version.as_str()],
                )
                .map_err(|e| e.to_string())?;
                c.execute(
                    "INSERT OR IGNORE INTO msgq_meta (key, value) VALUES (?1, ?2)",
                    ["created_by", created_by.as_str()],
                )
                .map_err(|e| e.to_string())?;
                // Retention deletes records, so the count of what a lagging consumer lost has to be
                // kept somewhere else: one row per prune, carrying the cumulative total removed.
                c.execute_batch(
                    "CREATE TABLE IF NOT EXISTS msgq_retention (                         topic TEXT NOT NULL,                         cut_offset INTEGER NOT NULL,                         removed_total INTEGER NOT NULL,                         removed_at INTEGER NOT NULL,                         PRIMARY KEY (topic, cut_offset))",
                )
                .map_err(|e| e.to_string())?;
                Ok(())
            })
            .map_err(Error::runtime)
    }

    /// Return the space a prune freed: rewrite the file, then fold the log back and empty it.
    ///
    /// Both halves matter on a WAL database. VACUUM rewrites the table into a fresh file, but it
    /// writes through the write-ahead log, so the pages it vacates are still in the log until a
    /// checkpoint folds them in. Best-effort throughout: VACUUM needs room for a second copy and an
    /// exclusive lock, and a failure changes nothing about the records, which are already gone.
    pub fn compact(&self) -> Result<(), Error> {
        self.db
            .with_conn(|c| -> Result<(), String> {
                c.execute_batch("VACUUM").map_err(|e| e.to_string())?;
                c.execute_batch("PRAGMA wal_checkpoint(TRUNCATE)")
                    .map_err(|e| e.to_string())
            })
            .map_err(Error::runtime)
    }

    /// One stream's size, and how old its oldest record is.
    pub fn stream_summaries(&self) -> Result<Vec<StreamSummary>, Error> {
        self.db
            .with_conn(|c| -> Result<Vec<StreamSummary>, String> {
                let mut stmt = c
                    .prepare(
                        "SELECT topic, COUNT(*), MIN(created_at), MAX(offset) \
                         FROM _honker_stream GROUP BY topic ORDER BY topic",
                    )
                    .map_err(|e| e.to_string())?;
                let rows = stmt
                    .query_map([], |r| {
                        Ok(StreamSummary {
                            topic: r.get(0)?,
                            records: r.get(1)?,
                            oldest_at: r.get(2)?,
                            newest_offset: r.get(3)?,
                        })
                    })
                    .map_err(|e| e.to_string())?;
                let mut out = Vec::new();
                for row in rows {
                    out.push(row.map_err(|e| e.to_string())?);
                }
                Ok(out)
            })
            .map_err(Error::runtime)
    }

    /// The highest offset in a stream whose record is older than `threshold`.
    ///
    /// Records are appended in time order, so "older than" is always a prefix: the answer is where
    /// that prefix ends. `None` means nothing in this stream is that old (and offset 0 is a real
    /// offset, so the distinction matters).
    pub fn prefix_end(&self, topic: &str, threshold: i64) -> Result<Option<i64>, Error> {
        self.db
            .with_conn(|c| -> Result<Option<i64>, String> {
                // The threshold is this process's own arithmetic, so it is safe to inline; the
                // topic comes from the caller and stays a parameter.
                c.query_row(
                    &format!(
                        "SELECT MAX(offset) FROM _honker_stream \
                         WHERE topic = ?1 AND created_at < {threshold}"
                    ),
                    [topic],
                    |r| r.get::<_, Option<i64>>(0),
                )
                .map_err(|e| e.to_string())
            })
            .map_err(Error::runtime)
    }

    /// Records in a stream at or below `cut`, ignoring the offsets named in `keep`.
    pub fn count_stream_prefix(&self, topic: &str, cut: i64, keep: &[i64]) -> Result<i64, Error> {
        self.db
            .with_conn(|c| -> Result<i64, String> {
                c.query_row(
                    &format!(
                        "SELECT COUNT(*) FROM _honker_stream WHERE topic = ?1 AND offset <= {cut}{}",
                        self.keep_clause(keep)
                    ),
                    [topic],
                    |r| r.get::<_, i64>(0),
                )
                .map_err(|e| e.to_string())
            })
            .map_err(Error::runtime)
    }

    /// Delete records in a stream at or below `cut`, ignoring the offsets named in `keep`.
    ///
    /// Removal runs in bounded statements so a large prefix never becomes one unbounded write. The
    /// offsets in `keep` come from this database, so a literal list is safe and needs no parameter
    /// juggling.
    pub fn delete_stream_prefix(
        &self,
        topic: &str,
        cut: i64,
        keep: &[i64],
        batch: usize,
    ) -> Result<Removal, Error> {
        let batch = batch.max(1) as i64;
        self.db
            .with_conn(|c| -> Result<Removal, String> {
                let mut removed = 0usize;
                let mut batches = 0usize;
                loop {
                    let sql = format!(
                        "DELETE FROM _honker_stream WHERE offset IN (\
                         SELECT offset FROM _honker_stream \
                         WHERE topic = ?1 AND offset <= {cut}{} LIMIT {batch})",
                        self.keep_clause(keep)
                    );
                    let deleted = c.execute(&sql, [topic]).map_err(|e| e.to_string())?;
                    if deleted == 0 {
                        break;
                    }
                    removed += deleted;
                    batches += 1;
                    if deleted < batch as usize {
                        break;
                    }
                }
                Ok(Removal { removed, batches })
            })
            .map_err(Error::runtime)
    }

    /// The oldest offset still retained in a stream, or `None` when the stream is empty.
    pub fn oldest_offset(&self, topic: &str) -> Result<Option<i64>, Error> {
        self.db
            .with_conn(|c| -> Result<Option<i64>, String> {
                c.query_row(
                    "SELECT MIN(offset) FROM _honker_stream WHERE topic = ?1",
                    [topic],
                    |r| r.get::<_, Option<i64>>(0),
                )
                .map_err(|e| e.to_string())
            })
            .map_err(Error::runtime)
    }

    /// Record that a prune removed records up to `cut`, `removed_total` of them in total.
    pub fn retention_record(&self, topic: &str, cut: i64, removed_total: i64) -> Result<(), Error> {
        self.db
            .with_conn(|c| -> Result<(), String> {
                c.execute(
                    &format!(
                        "INSERT INTO msgq_retention (topic, cut_offset, removed_total, removed_at) \
                         VALUES (?1, {cut}, {removed_total}, {}) \
                         ON CONFLICT(topic, cut_offset) DO UPDATE SET removed_total = {removed_total}, \
                         removed_at = {}",
                        Self::now_unix(),
                        Self::now_unix()
                    ),
                    [topic],
                )
                .map_err(|e| e.to_string())?;
                Ok(())
            })
            .map_err(Error::runtime)
    }

    /// How many records this stream removed after `cursor`. Zero when nothing was removed beyond it.
    pub fn retention_dropped(&self, topic: &str, cursor: i64) -> Result<i64, Error> {
        self.db
            .with_conn(|c| -> Result<i64, String> {
                let latest: i64 = c
                    .query_row(
                        "SELECT COALESCE(MAX(removed_total), 0) FROM msgq_retention WHERE topic = ?1",
                        [topic],
                        |r| r.get(0),
                    )
                    .map_err(|e| e.to_string())?;
                let at_or_before: i64 = c
                    .query_row(
                        &format!(
                            "SELECT COALESCE(MAX(removed_total), 0) FROM msgq_retention \
                             WHERE topic = ?1 AND cut_offset <= {cursor}"
                        ),
                        [topic],
                        |r| r.get(0),
                    )
                    .map_err(|e| e.to_string())?;
                Ok((latest - at_or_before).max(0))
            })
            .map_err(Error::runtime)
    }

    /// The newest retention watermark for a stream: the last offset pruned and the running total.
    pub fn retention_watermark(&self, topic: &str) -> Result<Option<(i64, i64)>, Error> {
        self.db
            .with_conn(|c| -> Result<Option<(i64, i64)>, String> {
                let mut stmt = c
                    .prepare(
                        "SELECT cut_offset, removed_total FROM msgq_retention WHERE topic = ?1 \
                         ORDER BY cut_offset DESC LIMIT 1",
                    )
                    .map_err(|e| e.to_string())?;
                let mut rows = stmt.query([topic]).map_err(|e| e.to_string())?;
                match rows.next().map_err(|e| e.to_string())? {
                    Some(row) => Ok(Some((
                        row.get::<_, i64>(0).map_err(|e| e.to_string())?,
                        row.get::<_, i64>(1).map_err(|e| e.to_string())?,
                    ))),
                    None => Ok(None),
                }
            })
            .map_err(Error::runtime)
    }

    fn now_unix() -> i64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0)
    }

    fn keep_clause(&self, keep: &[i64]) -> String {
        if keep.is_empty() {
            return String::new();
        }
        let list = keep
            .iter()
            .map(|offset| offset.to_string())
            .collect::<Vec<_>>()
            .join(",");
        format!(" AND offset NOT IN ({list})")
    }

    /// The newest presence record for every participant, which retention never removes.
    pub fn newest_presence_offsets(&self, topic: &str) -> Result<Vec<i64>, Error> {
        self.db
            .with_conn(|c| -> Result<Vec<i64>, String> {
                let mut stmt = c
                    .prepare(
                        "SELECT MAX(offset) FROM _honker_stream \
                         WHERE topic = ?1 AND json_valid(payload) \
                         GROUP BY COALESCE(json_extract(payload, '$.event.participant'), \
                                           json_extract(payload, '$.from'))",
                    )
                    .map_err(|e| e.to_string())?;
                let rows = stmt
                    .query_map([topic], |r| r.get::<_, i64>(0))
                    .map_err(|e| e.to_string())?;
                let mut out = Vec::new();
                for row in rows {
                    out.push(row.map_err(|e| e.to_string())?);
                }
                Ok(out)
            })
            .map_err(Error::runtime)
    }

    #[cfg(unix)]
    fn restrict_permissions(&self, path: &Path) -> Result<(), Error> {
        use std::os::unix::fs::PermissionsExt;

        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
            .map_err(|e| Error::runtime(format!("cannot restrict {}: {e}", path.display())))?;

        for suffix in ["-wal", "-shm"] {
            let mut side = path.as_os_str().to_os_string();
            side.push(suffix);
            let side = PathBuf::from(side);
            if side.exists() {
                let _ = std::fs::set_permissions(&side, std::fs::Permissions::from_mode(0o600));
            }
        }
        Ok(())
    }

    #[cfg(not(unix))]
    fn restrict_permissions(&self, _path: &Path) -> Result<(), Error> {
        Ok(())
    }

    /// Append one record to the stream named by `to`; returns the offset.
    pub fn publish(&self, to: &str, record: &serde_json::Value) -> Result<i64, Error> {
        Ok(self.db.stream(to).publish(record)?)
    }

    /// Read records after `offset` from one stream.
    pub fn read_since(
        &self,
        topic: &str,
        offset: i64,
        limit: i64,
    ) -> Result<Vec<StoredRecord>, Error> {
        let events = self.db.stream(topic).read_since(offset, limit)?;
        Ok(events
            .into_iter()
            .map(|event| StoredRecord {
                topic: event.topic,
                offset: event.offset,
                created_at: event.created_at,
                payload: event.payload,
            })
            .collect())
    }

    /// Highest offset in a stream, or 0 when it has no records.
    pub fn max_offset(&self, topic: &str) -> Result<i64, Error> {
        self.db
            .with_conn(|c| -> Result<i64, String> {
                c.query_row(
                    "SELECT COALESCE(MAX(offset), 0) FROM _honker_stream WHERE topic = ?1",
                    [topic],
                    |r| r.get::<_, i64>(0),
                )
                .map_err(|e| e.to_string())
            })
            .map_err(Error::runtime)
    }

    /// A consumer's saved offset, or `None` when it has never saved one.
    pub fn cursor_get(&self, consumer: &str, topic: &str) -> Result<Option<i64>, Error> {
        self.db
            .with_conn(|c| -> Result<Option<i64>, String> {
                let mut stmt = c
                    .prepare("SELECT offset FROM _honker_stream_consumers WHERE name = ?1 AND topic = ?2")
                    .map_err(|e| e.to_string())?;
                let mut rows = stmt
                    .query([consumer, topic])
                    .map_err(|e| e.to_string())?;
                match rows.next().map_err(|e| e.to_string())? {
                    Some(row) => Ok(Some(row.get::<_, i64>(0).map_err(|e| e.to_string())?)),
                    None => Ok(None),
                }
            })
            .map_err(Error::runtime)
    }

    /// Save a consumer offset. Monotonic: a lower value is ignored.
    pub fn cursor_set(&self, consumer: &str, topic: &str, offset: i64) -> Result<bool, Error> {
        Ok(self.db.stream(topic).save_offset(consumer, offset)?)
    }

    /// Clear a consumer offset (reset to "never saved").
    pub fn cursor_reset(&self, consumer: &str, topic: &str) -> Result<usize, Error> {
        self.db
            .with_conn(|c| -> Result<usize, String> {
                c.execute(
                    "DELETE FROM _honker_stream_consumers WHERE name = ?1 AND topic = ?2",
                    [consumer, topic],
                )
                .map_err(|e| e.to_string())
            })
            .map_err(Error::runtime)
    }

    /// Distinct topics with message count and highest offset.
    pub fn list_topics(&self) -> Result<Vec<(String, i64, i64)>, Error> {
        self.db
            .with_conn(|c| -> Result<Vec<(String, i64, i64)>, String> {
                let mut stmt = c
                    .prepare(
                        "SELECT topic, COUNT(*), COALESCE(MAX(offset), 0)                          FROM _honker_stream GROUP BY topic ORDER BY topic",
                    )
                    .map_err(|e| e.to_string())?;
                let rows = stmt
                    .query_map([], |r| {
                        Ok((
                            r.get::<_, String>(0)?,
                            r.get::<_, i64>(1)?,
                            r.get::<_, i64>(2)?,
                        ))
                    })
                    .map_err(|e| e.to_string())?;
                let mut out = Vec::new();
                for row in rows {
                    out.push(row.map_err(|e| e.to_string())?);
                }
                Ok(out)
            })
            .map_err(Error::runtime)
    }

    /// Consumer cursors, optionally filtered by topic.
    pub fn list_consumers(&self, topic: Option<&str>) -> Result<Vec<(String, String, i64)>, Error> {
        self.db
            .with_conn(|c| -> Result<Vec<(String, String, i64)>, String> {
                let mut stmt = c
                    .prepare(
                        "SELECT name, topic, offset FROM _honker_stream_consumers                          WHERE (?1 IS NULL OR topic = ?1) ORDER BY name, topic",
                    )
                    .map_err(|e| e.to_string())?;
                let rows = stmt
                    .query_map([topic], |r| {
                        Ok((
                            r.get::<_, String>(0)?,
                            r.get::<_, String>(1)?,
                            r.get::<_, i64>(2)?,
                        ))
                    })
                    .map_err(|e| e.to_string())?;
                let mut out = Vec::new();
                for row in rows {
                    out.push(row.map_err(|e| e.to_string())?);
                }
                Ok(out)
            })
            .map_err(Error::runtime)
    }

    /// Remove consumer cursors, optionally limited to one topic.
    pub fn delete_consumers(&self, name: &str, topic: Option<&str>) -> Result<usize, Error> {
        self.db
            .with_conn(|c| -> Result<usize, String> {
                c.execute(
                    "DELETE FROM _honker_stream_consumers                      WHERE name = ?1 AND (?2 IS NULL OR topic = ?2)",
                    [Some(name), topic],
                )
                .map_err(|e| e.to_string())
            })
            .map_err(Error::runtime)
    }

    /// The stored `(topic, payload)` for one offset, when it exists.
    pub fn record_at(&self, offset: i64) -> Result<Option<(String, String)>, Error> {
        self.db
            .with_conn(|c| -> Result<Option<(String, String)>, String> {
                let mut stmt = c
                    .prepare("SELECT topic, payload FROM _honker_stream WHERE offset = ?1")
                    .map_err(|e| e.to_string())?;
                let mut rows = stmt.query([offset]).map_err(|e| e.to_string())?;
                match rows.next().map_err(|e| e.to_string())? {
                    Some(row) => Ok(Some((
                        row.get::<_, String>(0).map_err(|e| e.to_string())?,
                        row.get::<_, String>(1).map_err(|e| e.to_string())?,
                    ))),
                    None => Ok(None),
                }
            })
            .map_err(Error::runtime)
    }

    /// Read one owned metadata value.
    pub fn meta(&self, key: &str) -> Result<Option<String>, Error> {
        self.db
            .with_conn(|c| -> Result<Option<String>, String> {
                let mut stmt = c
                    .prepare("SELECT value FROM msgq_meta WHERE key = ?1")
                    .map_err(|e| e.to_string())?;
                let mut rows = stmt.query([key]).map_err(|e| e.to_string())?;
                match rows.next().map_err(|e| e.to_string())? {
                    Some(row) => Ok(Some(row.get::<_, String>(0).map_err(|e| e.to_string())?)),
                    None => Ok(None),
                }
            })
            .map_err(Error::runtime)
    }

    /// Acquire an advisory lock.
    pub fn try_lock(
        &self,
        name: &str,
        owner: &str,
        ttl_s: i64,
    ) -> Result<Option<Box<dyn FollowLock>>, Error> {
        let held = self.db.try_lock(name, owner, ttl_s)?;
        Ok(held.map(|lock| Box::new(HonkerLock { lock }) as Box<dyn FollowLock>))
    }

    /// A wake source for a poll loop.
    pub fn wake(&self) -> Box<dyn Wake> {
        Box::new(HonkerWake {
            updates: self.db.update_events(),
        })
    }

    /// The local engine needs no per-address furniture: one table holds every stream.
    pub fn ensure_address(&self, _topic: &str) -> Result<(), Error> {
        Ok(())
    }
}

/// A held follow lock over the engine's advisory lock.
struct HonkerLock {
    lock: Lock,
}

impl FollowLock for HonkerLock {
    fn heartbeat(&self, ttl_s: i64) -> Result<(), Error> {
        Ok(self.lock.heartbeat(ttl_s).map(|_| ())?)
    }
}

/// A wake source over the engine's update watcher.
struct HonkerWake {
    updates: UpdateEvents,
}

impl Wake for HonkerWake {
    fn wait(&self, slice: std::time::Duration) -> Result<(), Error> {
        Ok(self.updates.recv_timeout(slice).map(|_| ())?)
    }
}

impl Bus for LocalStore {
    fn engine(&self) -> &'static str {
        "local"
    }
    fn wake(&self, _topics: &[&str]) -> Box<dyn Wake> {
        LocalStore::wake(self)
    }
    fn ensure_address(&self, topic: &str) -> Result<(), Error> {
        LocalStore::ensure_address(self, topic)
    }
    fn publish(&self, to: &str, record: &serde_json::Value) -> Result<i64, Error> {
        LocalStore::publish(self, to, record)
    }
    fn read_since(&self, topic: &str, offset: i64, limit: i64) -> Result<Vec<StoredRecord>, Error> {
        LocalStore::read_since(self, topic, offset, limit)
    }
    fn head(&self, topic: &str) -> Result<i64, Error> {
        LocalStore::max_offset(self, topic)
    }
    fn cursor_get(&self, consumer: &str, topic: &str) -> Result<Option<i64>, Error> {
        LocalStore::cursor_get(self, consumer, topic)
    }
    fn cursor_set(&self, consumer: &str, topic: &str, offset: i64) -> Result<bool, Error> {
        LocalStore::cursor_set(self, consumer, topic, offset)
    }
    fn cursor_reset(&self, consumer: &str, topic: &str) -> Result<usize, Error> {
        LocalStore::cursor_reset(self, consumer, topic)
    }
    fn prune_before(
        &self,
        topic: &str,
        cut: i64,
        keep: &[i64],
        batch: usize,
    ) -> Result<Removal, Error> {
        LocalStore::delete_stream_prefix(self, topic, cut, keep, batch)
    }
    fn list_addresses(&self) -> Result<Vec<(String, i64, i64)>, Error> {
        LocalStore::list_topics(self)
    }
    fn list_consumers(&self, topic: Option<&str>) -> Result<Vec<(String, String, i64)>, Error> {
        LocalStore::list_consumers(self, topic)
    }
    fn delete_consumers(&self, name: &str, topic: Option<&str>) -> Result<usize, Error> {
        LocalStore::delete_consumers(self, name, topic)
    }

    fn try_lock(
        &self,
        name: &str,
        owner: &str,
        ttl_s: i64,
    ) -> Result<Option<Box<dyn FollowLock>>, Error> {
        LocalStore::try_lock(self, name, owner, ttl_s)
    }
    fn meta(&self, key: &str) -> Result<Option<String>, Error> {
        LocalStore::meta(self, key)
    }
    fn compact(&self) -> Result<(), Error> {
        LocalStore::compact(self)
    }
    fn stream_summaries(&self) -> Result<Vec<StreamSummary>, Error> {
        LocalStore::stream_summaries(self)
    }
    fn prefix_end(&self, topic: &str, threshold: i64) -> Result<Option<i64>, Error> {
        LocalStore::prefix_end(self, topic, threshold)
    }
    fn count_stream_prefix(&self, topic: &str, cut: i64, keep: &[i64]) -> Result<i64, Error> {
        LocalStore::count_stream_prefix(self, topic, cut, keep)
    }
    fn newest_presence_offsets(&self, topic: &str) -> Result<Vec<i64>, Error> {
        LocalStore::newest_presence_offsets(self, topic)
    }
    fn oldest_offset(&self, topic: &str) -> Result<Option<i64>, Error> {
        LocalStore::oldest_offset(self, topic)
    }
    fn retention_record(&self, topic: &str, cut: i64, removed_total: i64) -> Result<(), Error> {
        LocalStore::retention_record(self, topic, cut, removed_total)
    }
    fn retention_watermark(&self, topic: &str) -> Result<Option<(i64, i64)>, Error> {
        LocalStore::retention_watermark(self, topic)
    }
    fn retention_dropped(&self, topic: &str, cursor: i64) -> Result<i64, Error> {
        LocalStore::retention_dropped(self, topic, cursor)
    }
    fn record_at(&self, offset: i64) -> Result<Option<(String, String)>, Error> {
        LocalStore::record_at(self, offset)
    }
}

/// An open message bus: whichever engine the resolved location selects.
///
/// Everything above this type is engine-neutral, so a command never learns which storage it is
/// talking to.
pub struct Store {
    inner: Box<dyn Bus>,
}

impl Store {
    /// Open the bus named by the location, creating a local file when it does not exist.
    ///
    /// Returns the store and whether the location was created here.
    pub fn open(target: &crate::config::Target) -> Result<(Self, bool), Error> {
        match target {
            crate::config::Target::Local(path) => {
                let (local, created) = LocalStore::open(path)?;
                Ok((
                    Store {
                        inner: Box::new(local),
                    },
                    created,
                ))
            }
            crate::config::Target::Shared(dsn) => {
                let pg = crate::pg::PgStore::connect(dsn)?;
                Ok((
                    Store {
                        inner: Box::new(pg),
                    },
                    false,
                ))
            }
        }
    }

    /// Which engine serves this bus, for reporting.
    pub fn engine(&self) -> &'static str {
        self.inner.engine()
    }

    /// The newest offset in a stream. Named for what callers ask for; the trait calls it head.
    pub fn max_offset(&self, topic: &str) -> Result<i64, Error> {
        self.inner.head(topic)
    }

    /// Every address with its record count and newest offset.
    pub fn list_topics(&self) -> Result<Vec<(String, i64, i64)>, Error> {
        self.inner.list_addresses()
    }

    /// Remove a prefix in bounded batches.
    pub fn delete_stream_prefix(
        &self,
        topic: &str,
        cut: i64,
        keep: &[i64],
        batch: usize,
    ) -> Result<Removal, Error> {
        self.inner.prune_before(topic, cut, keep, batch)
    }
}

impl Bus for Store {
    fn engine(&self) -> &'static str {
        self.inner.engine()
    }
    fn ensure_address(&self, topic: &str) -> Result<(), Error> {
        self.inner.ensure_address(topic)
    }
    fn publish(&self, to: &str, record: &serde_json::Value) -> Result<i64, Error> {
        self.inner.publish(to, record)
    }
    fn read_since(&self, topic: &str, offset: i64, limit: i64) -> Result<Vec<StoredRecord>, Error> {
        self.inner.read_since(topic, offset, limit)
    }
    fn head(&self, topic: &str) -> Result<i64, Error> {
        self.inner.head(topic)
    }
    fn cursor_get(&self, consumer: &str, topic: &str) -> Result<Option<i64>, Error> {
        self.inner.cursor_get(consumer, topic)
    }
    fn cursor_set(&self, consumer: &str, topic: &str, offset: i64) -> Result<bool, Error> {
        self.inner.cursor_set(consumer, topic, offset)
    }
    fn cursor_reset(&self, consumer: &str, topic: &str) -> Result<usize, Error> {
        self.inner.cursor_reset(consumer, topic)
    }
    fn prune_before(
        &self,
        topic: &str,
        cut: i64,
        keep: &[i64],
        batch: usize,
    ) -> Result<Removal, Error> {
        self.inner.prune_before(topic, cut, keep, batch)
    }
    fn list_addresses(&self) -> Result<Vec<(String, i64, i64)>, Error> {
        self.inner.list_addresses()
    }
    fn list_consumers(&self, topic: Option<&str>) -> Result<Vec<(String, String, i64)>, Error> {
        self.inner.list_consumers(topic)
    }
    fn delete_consumers(&self, name: &str, topic: Option<&str>) -> Result<usize, Error> {
        self.inner.delete_consumers(name, topic)
    }
    fn wake(&self, topics: &[&str]) -> Box<dyn Wake> {
        self.inner.wake(topics)
    }
    fn try_lock(
        &self,
        name: &str,
        owner: &str,
        ttl_s: i64,
    ) -> Result<Option<Box<dyn FollowLock>>, Error> {
        self.inner.try_lock(name, owner, ttl_s)
    }
    fn meta(&self, key: &str) -> Result<Option<String>, Error> {
        self.inner.meta(key)
    }
    fn compact(&self) -> Result<(), Error> {
        self.inner.compact()
    }
    fn stream_summaries(&self) -> Result<Vec<StreamSummary>, Error> {
        self.inner.stream_summaries()
    }
    fn prefix_end(&self, topic: &str, threshold: i64) -> Result<Option<i64>, Error> {
        self.inner.prefix_end(topic, threshold)
    }
    fn count_stream_prefix(&self, topic: &str, cut: i64, keep: &[i64]) -> Result<i64, Error> {
        self.inner.count_stream_prefix(topic, cut, keep)
    }
    fn newest_presence_offsets(&self, topic: &str) -> Result<Vec<i64>, Error> {
        self.inner.newest_presence_offsets(topic)
    }
    fn oldest_offset(&self, topic: &str) -> Result<Option<i64>, Error> {
        self.inner.oldest_offset(topic)
    }
    fn retention_record(&self, topic: &str, cut: i64, removed_total: i64) -> Result<(), Error> {
        self.inner.retention_record(topic, cut, removed_total)
    }
    fn retention_watermark(&self, topic: &str) -> Result<Option<(i64, i64)>, Error> {
        self.inner.retention_watermark(topic)
    }
    fn retention_dropped(&self, topic: &str, cursor: i64) -> Result<i64, Error> {
        self.inner.retention_dropped(topic, cursor)
    }
    fn record_at(&self, offset: i64) -> Result<Option<(String, String)>, Error> {
        self.inner.record_at(offset)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn temp_db() -> PathBuf {
        use std::sync::atomic::{AtomicU64, Ordering};
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let unique = COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "msgq-store-{}-{nanos}-{unique}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir.join("msgq.db")
    }

    fn publish(store: &dyn Bus, topic: &str, body: &str) {
        store
            .publish(
                topic,
                &json!({"v": 1, "type": "note", "from": "cli/a@b", "to": topic, "message": body}),
            )
            .unwrap();
    }

    #[test]
    fn removal_runs_in_bounded_batches() {
        let path = temp_db();
        let (store, _) = LocalStore::open(&path).unwrap();
        for i in 0..1_200 {
            publish(&store, "group/a", &format!("m{i}"));
        }
        let head = store.max_offset("group/a").unwrap();
        let removal = store
            .delete_stream_prefix("group/a", head, &[], 500)
            .unwrap();
        assert_eq!(removal.removed, 1_200, "every record is removed");
        assert_eq!(removal.batches, 3, "1200 records at 500 per statement");
        assert_eq!(store.oldest_offset("group/a").unwrap(), None);
    }

    #[test]
    fn the_watermark_counts_what_was_removed_after_a_cursor() {
        let path = temp_db();
        let (store, _) = LocalStore::open(&path).unwrap();
        for i in 0..10 {
            publish(&store, "group/a", &format!("m{i}"));
        }
        let cut = store.max_offset("group/a").unwrap() - 4;
        let removal = store
            .delete_stream_prefix("group/a", cut, &[], 500)
            .unwrap();
        store
            .retention_record("group/a", cut, removal.removed as i64)
            .unwrap();

        assert_eq!(store.retention_dropped("group/a", 0).unwrap(), 6);
        assert_eq!(
            store.retention_dropped("group/a", cut).unwrap(),
            0,
            "a cursor at the watermark lost nothing after it"
        );
        assert_eq!(
            store.retention_dropped("group/a", cut - 3).unwrap(),
            6,
            "a cursor inside the pruned range reports the whole removed range"
        );
    }

    #[test]
    fn the_watermark_is_cumulative_across_prunes() {
        let path = temp_db();
        let (store, _) = LocalStore::open(&path).unwrap();
        for i in 0..6 {
            publish(&store, "group/a", &format!("m{i}"));
        }
        let first = store.max_offset("group/a").unwrap() - 3;
        let r1 = store
            .delete_stream_prefix("group/a", first, &[], 500)
            .unwrap();
        store
            .retention_record("group/a", first, r1.removed as i64)
            .unwrap();

        let second = store.max_offset("group/a").unwrap();
        let r2 = store
            .delete_stream_prefix("group/a", second, &[], 500)
            .unwrap();
        store
            .retention_record("group/a", second, r1.removed as i64 + r2.removed as i64)
            .unwrap();

        assert_eq!(r1.removed + r2.removed, 6);
        assert_eq!(store.retention_dropped("group/a", 0).unwrap(), 6);
        assert_eq!(store.retention_dropped("group/a", first).unwrap(), 3);
    }
}
