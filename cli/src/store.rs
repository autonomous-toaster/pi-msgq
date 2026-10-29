//! Storage layer: a thin wrapper over honker's durable streams.
//!
//! All engine use is confined to this module so the record contract and
//! address grammar stay independent of the engine.

use std::path::{Path, PathBuf};

use honker::{Database, Lock, StreamEvent, UpdateEvents};

use crate::error::Error;
use crate::record::PROTOCOL_VERSION;

/// One stream's size, and how old its oldest record is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StreamSummary {
    pub topic: String,
    pub records: i64,
    /// When the oldest record still stored in this stream was written.
    pub oldest_at: i64,
    pub newest_offset: i64,
}

/// An open message bus.
pub struct Store {
    db: Database,
}

impl Store {
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
        let store = Store { db };
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
    /// The offsets in `keep` come from this database, so a literal list is safe and needs no
    /// parameter juggling.
    pub fn delete_stream_prefix(
        &self,
        topic: &str,
        cut: i64,
        keep: &[i64],
    ) -> Result<usize, Error> {
        self.db
            .with_conn(|c| -> Result<usize, String> {
                c.execute(
                    &format!(
                        "DELETE FROM _honker_stream WHERE topic = ?1 AND offset <= {cut}{}",
                        self.keep_clause(keep)
                    ),
                    [topic],
                )
                .map_err(|e| e.to_string())
            })
            .map_err(Error::runtime)
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
    ) -> Result<Vec<StreamEvent>, Error> {
        Ok(self.db.stream(topic).read_since(offset, limit)?)
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
    pub fn try_lock(&self, name: &str, owner: &str, ttl_s: i64) -> Result<Option<Lock>, Error> {
        Ok(self.db.try_lock(name, owner, ttl_s)?)
    }

    /// A raw update waker for custom poll loops.
    pub fn update_events(&self) -> UpdateEvents {
        self.db.update_events()
    }
}
