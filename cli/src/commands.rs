//! Command implementations.

use std::collections::BTreeMap;
use std::io::Read;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use honker::StreamEvent;
use serde_json::{Value, json};

use crate::address;
use crate::error::Error;
use crate::record;
use crate::store::Store;

/// Advisory lock lifetime for a follow; renewed while running.
const LOCK_TTL_S: i64 = 60;
/// How often to renew follow locks.
const LOCK_RENEW_EVERY: Duration = Duration::from_secs(30);
/// How long a follow waits for updates before checking for an interrupt.
const WAIT_SLICE: Duration = Duration::from_millis(250);
/// Records fetched per stream per follow iteration.
const FOLLOW_BATCH: i64 = 100;

/// Where a stateless read or a new subscription starts.
#[derive(Debug, Clone, Copy)]
pub enum FromSpec {
    /// From the earliest record.
    Start,
    /// From just after this offset.
    Offset(i64),
    /// From the current end of each stream.
    End,
}

/// Arguments for `publish`.
pub struct PublishArgs {
    pub address: String,
    pub message: Option<String>,
    pub from: Option<String>,
    pub kind: Option<String>,
    pub json: bool,
}

/// Action for `consumers`.
pub enum ConsumersAction {
    List,
    Rm { name: String, topic: Option<String> },
}

/// Action for `cursor`.
pub enum CursorAction {
    Get {
        consumer: String,
        topic: String,
    },
    Set {
        consumer: String,
        topic: String,
        offset: i64,
    },
    Reset {
        consumer: String,
        topic: String,
    },
}

/// Publish one message to an address.
pub fn publish(db_path: &Path, args: PublishArgs) -> Result<(), Error> {
    let canonical = address::canonicalize(&args.address)?;
    let value = build_record(&canonical, &args)?;
    let (store, _) = Store::open(db_path)?;
    let offset = store.publish(&canonical, &value)?;
    println!("{offset}");
    Ok(())
}

fn build_record(canonical: &str, args: &PublishArgs) -> Result<Value, Error> {
    if args.json {
        let raw = match args.message.as_deref() {
            Some("-") | None => read_stdin()?,
            Some(s) => s.to_string(),
        };
        let value: Value = serde_json::from_str(&raw)
            .map_err(|e| Error::usage(format!("invalid JSON record: {e}")))?;
        let rec = record::parse_and_validate(&value)?;
        let to = address::canonicalize(&rec.to)?;
        if to != canonical {
            return Err(Error::usage(format!(
                "record 'to' ({to}) does not match address ({canonical})"
            )));
        }
        Ok(value)
    } else {
        let kind = args.kind.clone().unwrap_or_else(|| "prompt".to_string());
        if kind == "event" {
            return Err(Error::usage("type 'event' requires --json"));
        }
        let message = match args.message.as_deref() {
            Some("-") => read_stdin()?,
            Some(s) => s.to_string(),
            None => {
                return Err(Error::usage(
                    "missing message; pass a message argument or '-' to read stdin",
                ));
            }
        };
        let from = args.from.clone().unwrap_or_else(record::default_sender);
        let rec = record::Record::message(&kind, &from, canonical, message);
        record::validate(&rec)?;
        record::to_value(&rec)
    }
}

/// Stateless read: never creates or advances a consumer cursor.
pub fn read(
    db_path: &Path,
    addresses: &[String],
    from: FromSpec,
    limit: i64,
    json: bool,
) -> Result<(), Error> {
    let targets = address::canonicalize_all(addresses)?;
    let (store, _) = Store::open(db_path)?;
    let limit = limit.max(1);

    let mut events: Vec<StreamEvent> = Vec::new();
    for topic in &targets {
        let offset = match from {
            FromSpec::Start => 0,
            FromSpec::Offset(n) => n,
            FromSpec::End => store.max_offset(topic)?,
        };
        events.extend(store.read_since(topic, offset, limit)?);
    }
    events.sort_by(|a, b| a.offset.cmp(&b.offset).then_with(|| a.topic.cmp(&b.topic)));
    events.truncate(limit as usize);

    for event in &events {
        render(event, json);
    }
    Ok(())
}

/// Cursor-based follow: resumes a consumer and advances its offsets.
pub fn follow(
    db_path: &Path,
    addresses: &[String],
    consumer: &str,
    from: FromSpec,
    json: bool,
) -> Result<(), Error> {
    let targets = address::canonicalize_all(addresses)?;
    let (store, _) = Store::open(db_path)?;
    let owner = format!("msgq:{}:{}", std::process::id(), nanos());

    let mut locks = Vec::new();
    for topic in &targets {
        let name = format!("msgq:follow:{consumer}:{topic}");
        match store.try_lock(&name, &owner, LOCK_TTL_S)? {
            Some(lock) => locks.push(lock),
            None => {
                return Err(Error::runtime(format!(
                    "consumer '{consumer}' is already following '{topic}' in another process"
                )));
            }
        }
    }

    let mut offsets: BTreeMap<String, i64> = BTreeMap::new();
    for topic in &targets {
        let start = match from {
            FromSpec::Start => 0,
            FromSpec::Offset(n) => n,
            FromSpec::End => match store.cursor_get(consumer, topic)? {
                Some(saved) => saved,
                None => store.max_offset(topic)?,
            },
        };
        offsets.insert(topic.clone(), start);
    }

    let interrupted = Arc::new(AtomicBool::new(false));
    {
        let flag = Arc::clone(&interrupted);
        let _ = ctrlc::set_handler(move || flag.store(true, Ordering::SeqCst));
    }

    let updates = store.update_events();
    let mut last_renew = Instant::now();

    while !interrupted.load(Ordering::SeqCst) {
        let mut batch: Vec<StreamEvent> = Vec::new();
        for topic in &targets {
            let offset = *offsets.get(topic).unwrap_or(&0);
            batch.extend(store.read_since(topic, offset, FOLLOW_BATCH)?);
        }
        batch.sort_by(|a, b| a.offset.cmp(&b.offset).then_with(|| a.topic.cmp(&b.topic)));

        if batch.is_empty() {
            if last_renew.elapsed() >= LOCK_RENEW_EVERY {
                for lock in &locks {
                    let _ = lock.heartbeat(LOCK_TTL_S);
                }
                last_renew = Instant::now();
            }
            updates.recv_timeout(WAIT_SLICE)?;
            continue;
        }

        for event in batch {
            store.cursor_set(consumer, &event.topic, event.offset)?;
            offsets.insert(event.topic.clone(), event.offset);
            render(&event, json);
        }
    }

    Ok(())
}

/// List distinct topics with message counts and highest offsets.
pub fn topics(db_path: &Path, json_out: bool) -> Result<(), Error> {
    let (store, _) = Store::open(db_path)?;
    let rows = store.list_topics()?;
    if json_out {
        let array: Vec<Value> = rows
            .iter()
            .map(
                |(topic, count, max)| json!({"topic": topic, "messages": count, "max_offset": max}),
            )
            .collect();
        println!("{}", serde_json::to_string(&array)?);
    } else {
        for (topic, count, max) in rows {
            println!("{topic}\t{count}\t{max}");
        }
    }
    Ok(())
}

/// List or remove consumer cursors.
pub fn consumers(
    db_path: &Path,
    topic: Option<&str>,
    action: ConsumersAction,
    json_out: bool,
) -> Result<(), Error> {
    let topic = match topic {
        Some(t) => Some(address::canonicalize(t)?),
        None => None,
    };
    let (store, _) = Store::open(db_path)?;

    match action {
        ConsumersAction::List => {
            let rows = store.list_consumers(topic.as_deref())?;
            if json_out {
                let array: Vec<Value> = rows
                    .iter()
                    .map(|(name, topic, offset)| {
                        json!({"consumer": name, "topic": topic, "offset": offset})
                    })
                    .collect();
                println!("{}", serde_json::to_string(&array)?);
            } else {
                for (name, topic, offset) in rows {
                    println!("{name}\t{topic}\t{offset}");
                }
            }
        }
        ConsumersAction::Rm {
            name,
            topic: rm_topic,
        } => {
            let topic = match rm_topic {
                Some(t) => Some(address::canonicalize(&t)?),
                None => topic,
            };
            let removed = store.delete_consumers(&name, topic.as_deref())?;
            println!("{removed}");
        }
    }
    Ok(())
}

/// Read, set, or reset one consumer cursor.
pub fn cursor(db_path: &Path, action: CursorAction) -> Result<(), Error> {
    let (store, _) = Store::open(db_path)?;
    match action {
        CursorAction::Get { consumer, topic } => {
            let topic = address::canonicalize(&topic)?;
            let offset = store.cursor_get(&consumer, &topic)?.unwrap_or(0);
            println!("{offset}");
        }
        CursorAction::Set {
            consumer,
            topic,
            offset,
        } => {
            let topic = address::canonicalize(&topic)?;
            let changed = store.cursor_set(&consumer, &topic, offset)?;
            println!("{}", if changed { "ok" } else { "ignored" });
        }
        CursorAction::Reset { consumer, topic } => {
            let topic = address::canonicalize(&topic)?;
            let removed = store.cursor_reset(&consumer, &topic)?;
            println!("{removed}");
        }
    }
    Ok(())
}

/// Report database path, size, contents, and protocol version.
pub fn db_info(db_path: &Path) -> Result<(), Error> {
    let existed_before = db_path.exists();
    let (store, created) = Store::open(db_path)?;

    let size = std::fs::metadata(db_path).map(|m| m.len()).unwrap_or(0);
    let version = store
        .meta("protocol_version")?
        .unwrap_or_else(|| "unknown".to_string());
    let created_by = store
        .meta("created_by")?
        .unwrap_or_else(|| "unknown".to_string());
    let topics = store.list_topics()?;
    let consumers = store.list_consumers(None)?;
    let messages: i64 = topics.iter().map(|(_, count, _)| *count).sum();

    println!("path:             {}", crate::config::display(db_path));
    println!(
        "existed_before:   {}",
        if existed_before { "yes" } else { "no" }
    );
    println!("created:          {}", if created { "yes" } else { "no" });
    println!("size_bytes:       {size}");
    println!("protocol_version: {version}");
    println!("created_by:       {created_by}");
    println!("topics:           {}", topics.len());
    println!("messages:         {messages}");
    println!("consumers:        {}", consumers.len());

    if !topics.is_empty() {
        println!("\nstreams:");
        for (topic, count, max) in &topics {
            println!("  {topic}\t{count}\t{max}");
        }
    }
    if !consumers.is_empty() {
        println!("\nconsumers:");
        for (name, topic, offset) in &consumers {
            println!("  {name}\t{topic}\t{offset}");
        }
    }
    Ok(())
}

fn read_stdin() -> Result<String, Error> {
    let mut buf = String::new();
    std::io::stdin()
        .read_to_string(&mut buf)
        .map_err(|e| Error::runtime(format!("cannot read stdin: {e}")))?;
    Ok(buf.trim_end().to_string())
}

fn nanos() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0)
}

/// Render one event as a raw JSON line or a human-readable line.
fn render(event: &StreamEvent, json_out: bool) {
    if json_out {
        println!("{}", String::from_utf8_lossy(&event.payload));
        return;
    }

    let value: Value = serde_json::from_slice(&event.payload).unwrap_or(Value::Null);
    let from = value.get("from").and_then(Value::as_str).unwrap_or("?");
    let kind = value.get("type").and_then(Value::as_str).unwrap_or("?");
    let body = if kind == "event" {
        value
            .get("event")
            .map(|e| e.to_string())
            .unwrap_or_default()
    } else {
        value
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string()
    };

    println!(
        "{:>7}  {}  {:<24}  {:<9}  {:<20}  {}",
        event.offset,
        format_unix(event.created_at),
        event.topic,
        kind,
        from,
        body
    );
}

/// Format a unix timestamp (seconds) as an ISO-8601 UTC string.
fn format_unix(ts: i64) -> String {
    let days = ts.div_euclid(86_400);
    let seconds = ts.rem_euclid(86_400);
    let (year, month, day) = civil_from_days(days);
    let hour = seconds / 3600;
    let minute = (seconds % 3600) / 60;
    let second = seconds % 60;
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}Z")
}

/// Days since the unix epoch to a civil (year, month, day), UTC.
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let year = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if month <= 2 { year + 1 } else { year }, month, day)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_epoch() {
        assert_eq!(format_unix(0), "1970-01-01T00:00:00Z");
        assert_eq!(format_unix(1_700_000_000), "2023-11-14T22:13:20Z");
    }
}
