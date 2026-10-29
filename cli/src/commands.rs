//! Command implementations.

use std::collections::{BTreeMap, BTreeSet};
use std::io::Read;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use honker::StreamEvent;
use serde_json::{Value, json};

use crate::address;
use crate::error::Error;
use crate::presence::{self, Roster};
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
    /// `--ack`: `true` for every phase, or a comma-separated phase list.
    pub ack: Option<String>,
    /// `--reply-to`: the offset of the message this one answers, or its id.
    pub reply_to: Option<String>,
    /// Print the record's id instead of its offset.
    pub print_id: bool,
    pub await_ack: bool,
    pub timeout: u64,
}

/// Arguments for `ask`.
pub struct AskArgs {
    pub address: String,
    pub message: Option<String>,
    pub from: Option<String>,
    /// The stream the answer arrives on; defaults to this sender's own address.
    pub on: Option<String>,
    pub timeout: u64,
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

    // When awaiting, record where the reply stream ends *before* publishing, so an
    // acknowledgement that arrives immediately is still reported.
    let reply = if args.await_ack {
        let sender = value
            .get("from")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        let stream = address::canonicalize(&sender)?;
        let from_offset = store.max_offset(&stream)?;
        Some((stream, from_offset))
    } else {
        None
    };

    let offset = store.publish(&canonical, &value)?;
    // A bare offset stays the default: it is what scripts already parse.
    if args.print_id {
        println!("{}", address::record_id(&canonical, offset));
    } else {
        println!("{offset}");
    }

    if let Some((stream, from_offset)) = reply {
        await_terminal(&store, &stream, from_offset, offset, args.timeout, false)?;
    }
    Ok(())
}

/// Wait for acknowledgements of an already published offset.
pub fn await_ack(
    db_path: &Path,
    offset: i64,
    on: Option<&str>,
    timeout_s: u64,
    json_out: bool,
) -> Result<(), Error> {
    let (store, _) = Store::open(db_path)?;
    // Without an explicit address, ask whoever sent the message: acknowledgements are
    // addressed to the sender, not to the stream the message was published to.
    let stream = match on {
        Some(value) => address::canonicalize(value)?,
        None => match sender_of(&store, offset)? {
            Some(sender) => address::canonicalize(&sender)?,
            None => address::canonicalize(&record::default_sender())?,
        },
    };
    // Re-attaching has no snapshot to take, so the reply stream is read from its start.
    await_terminal(&store, &stream, 0, offset, timeout_s, json_out)
}

/// `msgq ask`: publish a question and wait for the answer.
///
/// Ends on a reply, on a terminal acknowledgement, or on the wait window. None of
/// the three is an error: in all of them the peer is fine.
pub fn ask(db_path: &Path, args: AskArgs) -> Result<(), Error> {
    let canonical = address::canonicalize(&args.address)?;
    let message = match args.message.as_deref() {
        Some("-") => read_stdin()?,
        Some(text) => text.to_string(),
        None => {
            return Err(Error::usage(
                "missing message; pass a question or '-' to read stdin",
            ));
        }
    };
    let from = args.from.clone().unwrap_or_else(record::default_sender);
    let mut rec = record::Record::message("prompt", &from, &canonical, message);
    // An ask needs both: the recipient should be able to answer, and the sender needs
    // the lifecycle to know the peer finished without ever answering.
    rec.meta = Some(json!({ "ack": true, "reply": true }));
    record::validate(&rec)?;
    let value = record::to_value(&rec)?;

    let (store, _) = Store::open(db_path)?;
    // The answer comes back to the sender, so record where that stream ends before asking.
    let stream = match args.on.as_deref() {
        Some(value) => address::canonicalize(value)?,
        None => address::canonicalize(&from)?,
    };
    let from_offset = store.max_offset(&stream)?;

    let offset = store.publish(&canonical, &value)?;
    println!("{offset}");

    await_reply(
        &store,
        &stream,
        from_offset,
        offset,
        args.timeout,
        args.json,
    )
}

/// Print the answer to an ask, or report why there is none yet.
fn await_reply(
    store: &Store,
    stream: &str,
    from_offset: i64,
    reply_to: i64,
    timeout_s: u64,
    json_out: bool,
) -> Result<(), Error> {
    let updates = store.update_events();
    let started = Instant::now();
    let mut cursor = from_offset;

    loop {
        let events = store.read_since(stream, cursor, 200)?;
        if events.is_empty() {
            if timeout_s > 0 && started.elapsed() >= Duration::from_secs(timeout_s) {
                report_outcome(
                    json_out,
                    json!({"outcome": "running", "reply_to": reply_to}),
                    "still running",
                );
                return Ok(());
            }
            updates.recv_timeout(Duration::from_millis(500))?;
            continue;
        }

        for event in events {
            cursor = event.offset;
            let record: Value = serde_json::from_slice(&event.payload).unwrap_or(Value::Null);
            if record.get("reply_to").and_then(Value::as_i64) != Some(reply_to) {
                continue;
            }

            // Acknowledgements share the reply reference, so they are told apart by shape.
            if let Some(ack) = ack_event(&record) {
                let phase = ack
                    .get("phase")
                    .and_then(Value::as_str)
                    .unwrap_or("unknown");
                let delivery = ack.get("delivery").and_then(Value::as_str);
                let terminal =
                    phase == "completed" || (phase == "received" && delivery == Some("display"));
                if terminal {
                    report_outcome(
                        json_out,
                        json!({"outcome": "finished", "phase": phase, "reply_to": reply_to}),
                        &format!("finished without replying ({phase})"),
                    );
                    return Ok(());
                }
                continue;
            }

            if json_out {
                println!("{}", String::from_utf8_lossy(&event.payload));
            } else {
                println!(
                    "{}",
                    record.get("message").and_then(Value::as_str).unwrap_or("")
                );
            }
            return Ok(());
        }
    }
}

fn report_outcome(json_out: bool, value: Value, human: &str) {
    if json_out {
        match serde_json::to_string(&value) {
            Ok(line) => println!("{line}"),
            Err(_) => println!("{human}"),
        }
    } else {
        println!("{human}");
    }
}

/// The sender recorded on the message at `offset`, when the offset is known.
fn sender_of(store: &Store, offset: i64) -> Result<Option<String>, Error> {
    let Some((_topic, payload)) = store.record_at(offset)? else {
        return Ok(None);
    };
    let value: Value = serde_json::from_str(&payload).unwrap_or(Value::Null);
    Ok(value
        .get("from")
        .and_then(Value::as_str)
        .map(|sender| sender.to_string()))
}

/// Print acknowledgements of `reply_to` until a terminal one arrives or the window closes.
fn await_terminal(
    store: &Store,
    stream: &str,
    from_offset: i64,
    reply_to: i64,
    timeout_s: u64,
    json_out: bool,
) -> Result<(), Error> {
    let updates = store.update_events();
    let started = Instant::now();
    let mut cursor = from_offset;

    loop {
        let events = store.read_since(stream, cursor, 200)?;
        if events.is_empty() {
            if timeout_s > 0 && started.elapsed() >= Duration::from_secs(timeout_s) {
                return Err(Error::timeout(format!(
                    "no terminal acknowledgement for offset {reply_to} within {timeout_s}s"
                )));
            }
            updates.recv_timeout(Duration::from_millis(500))?;
            continue;
        }

        for event in events {
            cursor = event.offset;
            let record: Value = serde_json::from_slice(&event.payload).unwrap_or(Value::Null);
            if record.get("reply_to").and_then(Value::as_i64) != Some(reply_to) {
                continue;
            }
            let Some(ack) = record.get("event") else {
                continue;
            };
            if ack.get("kind").and_then(Value::as_str) != Some("ack") {
                continue;
            }

            let phase = ack
                .get("phase")
                .and_then(Value::as_str)
                .unwrap_or("unknown");
            let delivery = ack.get("delivery").and_then(Value::as_str);
            let outcome = ack.get("outcome").and_then(Value::as_str);

            if json_out {
                println!(
                    "{}",
                    serde_json::to_string(&json!({
                        "phase": phase,
                        "reply_to": reply_to,
                        "from": record.get("from").and_then(Value::as_str),
                        "delivery": delivery,
                        "outcome": outcome,
                        "offset": event.offset,
                    }))?
                );
            } else {
                let mut line = phase.to_string();
                if let Some(delivery) = delivery {
                    line.push_str(&format!(" delivery={delivery}"));
                }
                if let Some(outcome) = outcome {
                    line.push_str(&format!(" outcome={outcome}"));
                }
                println!("{line}");
            }

            // Terminal: completion, or a received acknowledgement for a display delivery.
            if phase == "completed" || (phase == "received" && delivery == Some("display")) {
                return Ok(());
            }
        }
    }
}

fn build_record(canonical: &str, args: &PublishArgs) -> Result<Value, Error> {
    if args.json {
        let raw = match args.message.as_deref() {
            Some("-") | None => read_stdin()?,
            Some(s) => s.to_string(),
        };
        let mut value: Value = serde_json::from_str(&raw)
            .map_err(|e| Error::usage(format!("invalid JSON record: {e}")))?;
        let rec = record::parse_and_validate(&value)?;
        let to = address::canonicalize(&rec.to)?;
        if to != canonical {
            return Err(Error::usage(format!(
                "record 'to' ({to}) does not match address ({canonical})"
            )));
        }
        // A record supplied whole may carry its own `reply_to`; the flag overrides it.
        if let Some(raw) = &args.reply_to {
            value["reply_to"] = json!(resolve_reply_to(raw, &rec.from)?);
        }
        note_unknown_type(&rec.kind);
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
        let mut rec = record::Record::message(&kind, &from, canonical, message);
        if let Some(phases) = &args.ack {
            let ack = if phases == "true" {
                Value::Bool(true)
            } else {
                Value::String(phases.clone())
            };
            rec.meta = Some(json!({ "ack": ack }));
        }
        rec.reply_to = args
            .reply_to
            .as_deref()
            .map(|raw| resolve_reply_to(raw, &from))
            .transpose()?;
        record::validate(&rec)?;
        note_unknown_type(&kind);
        record::to_value(&rec)
    }
}

/// Say so when a record names a type this version does not know.
///
/// The wire accepts it — a reader that does not know a type still has a record — so this is a note
/// rather than a refusal. It is worth one line on stderr because the usual cause is a typo.
fn note_unknown_type(kind: &str) {
    if !record::is_known_type(kind) {
        eprintln!(
            "msgq: type '{kind}' is not one this version knows; publishing anyway, and a reader \
             will deliver it as content"
        );
    }
}

/// Resolve `--reply-to`: a bare offset, or an id naming the record being answered.
///
/// An id carries the address whose stream holds the record, and a reference is an
/// offset in the stream named by the replying record's own sender, so the address
/// in the id has to be that sender.
fn resolve_reply_to(raw: &str, from: &str) -> Result<i64, Error> {
    let trimmed = raw.trim();
    if let Ok(offset) = trimmed.parse::<i64>() {
        return Ok(offset);
    }
    let (address, offset) = address::parse_record_id(trimmed)?;
    let sender = address::canonicalize(from)?;
    if address != sender {
        let id = address::record_id(&address, offset);
        return Err(Error::usage(format!(
            "cannot answer {id}: this record is sent from {sender}, and a reference points into the sender's own stream"
        )));
    }
    Ok(offset)
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

/// The presence tail, oldest first: what the roster and the collection commands both read.
fn presence_tail(store: &Store) -> Result<Vec<(i64, presence::PresenceRecord)>, Error> {
    let head = store.max_offset(presence::PRESENCE_ADDRESS)?;
    let start = (head - presence::SCAN_RECORDS).max(0);
    let events = store.read_since(presence::PRESENCE_ADDRESS, start, presence::SCAN_RECORDS)?;

    let mut out = Vec::new();
    for event in events {
        let Ok(payload) = serde_json::from_slice::<Value>(&event.payload) else {
            continue;
        };
        let Some(record) = presence::parse_presence(&payload) else {
            continue;
        };
        out.push((event.created_at, record));
    }
    Ok(out)
}

/// Who is live, by participant. The roster and cursor collection agree by construction.
fn live_participants(store: &Store) -> Result<BTreeSet<String>, Error> {
    let now = unix_now();
    let mut live: BTreeSet<String> = BTreeSet::new();
    for (created_at, record) in presence_tail(store)? {
        if presence::is_retraction(&record.state) {
            live.remove(&record.participant);
        } else if created_at + record.ttl > now {
            live.insert(record.participant);
        }
    }
    Ok(live)
}

/// List live participants from the presence address.
pub fn sessions(db_path: &Path, json_out: bool) -> Result<(), Error> {
    let (store, _) = Store::open(db_path)?;
    let now = unix_now();

    // One entry per participant. Stream order is the truth: the last record for a participant is
    // its current state, a retraction removes it, and a later record brings it back.
    let mut best: BTreeMap<String, Roster> = BTreeMap::new();
    for (created_at, record) in presence_tail(&store)? {
        if presence::is_retraction(&record.state) {
            best.remove(&record.participant);
            continue;
        }
        let expires_at = created_at + record.ttl;
        if expires_at <= now {
            continue;
        }
        best.insert(
            record.participant.clone(),
            Roster {
                address: record.address.clone(),
                kind: presence::kind_of(&record.address),
                state: record.state.clone(),
                alias: record.alias.clone(),
                expires_at,
            },
        );
    }

    let mut entries: Vec<Roster> = best.into_values().collect();
    entries.sort_by(|a, b| {
        a.address
            .cmp(&b.address)
            .then_with(|| a.state.cmp(&b.state))
            .then_with(|| b.expires_at.cmp(&a.expires_at))
    });

    if json_out {
        for entry in &entries {
            println!(
                "{}",
                serde_json::to_string(&json!({
                    "address": entry.address,
                    "kind": entry.kind,
                    "state": entry.state,
                    "alias": entry.alias,
                    "expires_at": entry.expires_at,
                    "expires_in": (entry.expires_at - now).max(0),
                }))?
            );
        }
    } else {
        for entry in &entries {
            println!(
                "{}\t{}\t{}\t{}\t{}",
                entry.address,
                entry.kind,
                entry.state,
                entry.alias.clone().unwrap_or_default(),
                (entry.expires_at - now).max(0)
            );
        }
    }
    Ok(())
}

/// Parse an age such as `7d`, `36h`, `90m` or `45s` into seconds.
fn parse_age(raw: &str) -> Result<i64, Error> {
    let trimmed = raw.trim();
    let (digits, unit) = if let Some(rest) = trimmed.strip_suffix('d') {
        (rest, 86_400)
    } else if let Some(rest) = trimmed.strip_suffix('h') {
        (rest, 3_600)
    } else if let Some(rest) = trimmed.strip_suffix('m') {
        (rest, 60)
    } else if let Some(rest) = trimmed.strip_suffix('s') {
        (rest, 1)
    } else {
        return Err(Error::usage(format!(
            "'{trimmed}' is not an age; expected a number and one of d, h, m, s (for example 7d)"
        )));
    };
    let count: i64 = digits.trim().parse().map_err(|_| {
        Error::usage(format!(
            "'{trimmed}' is not an age; expected a number and one of d, h, m, s (for example 7d)"
        ))
    })?;
    if count <= 0 {
        return Err(Error::usage(format!("'{trimmed}' must be a positive age")));
    }
    Ok(count * unit)
}

/// What `msgq prune` was asked to do.
pub struct PruneArgs {
    pub older_than: String,
    pub apply: bool,
}

/// Remove records older than an age, never removing one a consumer has not passed.
pub fn prune(db_path: &Path, args: PruneArgs) -> Result<(), Error> {
    let age = parse_age(&args.older_than)?;
    let (store, _) = Store::open(db_path)?;
    let threshold = unix_now() - age;

    // The oldest cursor in a stream pins everything from that record onwards: retention may only
    // take what every consumer of that stream has already passed.
    let mut pins: BTreeMap<String, (i64, BTreeSet<String>)> = BTreeMap::new();
    for (name, topic, offset) in store.list_consumers(None)? {
        let entry = pins.entry(topic).or_insert((offset, BTreeSet::new()));
        entry.0 = entry.0.min(offset);
        entry.1.insert(name);
    }

    // Presence is a window, not a history: the newest record of every participant is what the
    // roster reads, so it stays even when it is older than the age asked for.
    let presence_keep = store.newest_presence_offsets(presence::PRESENCE_ADDRESS)?;

    let mut planned = 0i64;
    let mut removed = 0i64;
    let mut pinned_total = 0i64;
    let mut report: Vec<String> = Vec::new();

    for stream in store.stream_summaries()? {
        // A cursor names the last record its consumer has taken responsibility for, so the
        // removable range is inclusive at both ends.
        let Some(end) = store.prefix_end(&stream.topic, threshold)? else {
            continue;
        };
        let mut cut = end;
        if let Some((pin, _)) = pins.get(&stream.topic) {
            let capped = cut.min(*pin);
            pinned_total += cut - capped;
            cut = capped;
        }
        if cut < 0 {
            continue;
        }

        let keep: &[i64] = if stream.topic == presence::PRESENCE_ADDRESS {
            &presence_keep
        } else {
            &[]
        };
        let count = store.count_stream_prefix(&stream.topic, cut, keep)?;
        if count <= 0 {
            continue;
        }
        planned += count;
        if args.apply {
            removed += store.delete_stream_prefix(&stream.topic, cut, keep)? as i64;
        }
        report.push(format!(
            "  {}\t{} of {}",
            stream.topic, count, stream.records
        ));
    }

    if report.is_empty() {
        println!("nothing is older than {}", args.older_than);
        return Ok(());
    }
    for line in &report {
        println!("{line}");
    }
    if pinned_total > 0 {
        // A bus accumulates a dead cursor per session per address, so naming all of them would be a
        // wall of text. Name a few, say how many more there are, and let `db info` be the index.
        let names: BTreeSet<&str> = pins
            .values()
            .flat_map(|(_, names)| names.iter().map(String::as_str))
            .collect();
        let shown: Vec<&str> = names.iter().take(5).copied().collect();
        let more = names.len().saturating_sub(shown.len());
        let tail = if more > 0 {
            format!(" and {more} more")
        } else {
            String::new()
        };
        eprintln!(
            "msgq: {pinned_total} record(s) kept because a consumer has not passed them ({}{tail})",
            shown.join(", ")
        );
    }
    if !presence_keep.is_empty() {
        eprintln!(
            "msgq: the newest presence record of {} participant(s) is kept",
            presence_keep.len()
        );
    }
    if args.apply {
        println!("removed {removed} record(s)");
        if removed > 0 {
            // The pages a prune frees are reused, not returned; compacting is what makes the file
            // smaller again, and it is allowed to fail.
            match store.compact() {
                Ok(()) => {}
                Err(error) => eprintln!("msgq: the file could not be compacted: {error}"),
            }
        }
    } else {
        println!("would remove {planned} record(s); pass --apply to remove them");
    }
    Ok(())
}

/// A session-shaped consumer cursor: `<alias>#<uuid>` or a bare `<uuid>`.
fn session_id_of(consumer: &str) -> Option<&str> {
    let candidate = consumer
        .rsplit_once('#')
        .map(|(_, id)| id)
        .unwrap_or(consumer);
    is_session_id(candidate).then_some(candidate)
}

fn is_session_id(value: &str) -> bool {
    value.len() == 36
        && value.char_indices().all(|(index, ch)| match index {
            8 | 13 | 18 | 23 => ch == '-',
            _ => ch.is_ascii_hexdigit(),
        })
}

/// Remove the cursors of sessions that are no longer live.
///
/// Only session-shaped consumer names are considered: a cursor named by a script is that script's
/// business, and collecting it would move its position without anybody asking.
pub fn consumers_gc(db_path: &Path, apply: bool) -> Result<(), Error> {
    let (store, _) = Store::open(db_path)?;
    let live = live_participants(&store)?;

    let mut candidates: Vec<(String, String)> = Vec::new();
    for (name, topic, _) in store.list_consumers(None)? {
        let Some(id) = session_id_of(&name) else {
            continue;
        };
        if live.contains(id) {
            continue;
        }
        candidates.push((name, topic));
    }

    if !apply {
        println!("{}", candidates.len());
        eprintln!("msgq: pass --apply to remove them");
        return Ok(());
    }

    let mut removed = 0usize;
    for (name, topic) in &candidates {
        removed += store.delete_consumers(name, Some(topic))?;
    }
    println!("{removed}");
    Ok(())
}

fn unix_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
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
    let now = unix_now();

    let size = std::fs::metadata(db_path).map(|m| m.len()).unwrap_or(0);
    let version = store
        .meta("protocol_version")?
        .unwrap_or_else(|| "unknown".to_string());
    let created_by = store
        .meta("created_by")?
        .unwrap_or_else(|| "unknown".to_string());
    let streams = store.stream_summaries()?;
    let consumers = store.list_consumers(None)?;
    let messages: i64 = streams.iter().map(|stream| stream.records).sum();

    println!("path:             {}", crate::config::display(db_path));
    println!(
        "existed_before:   {}",
        if existed_before { "yes" } else { "no" }
    );
    println!("created:          {}", if created { "yes" } else { "no" });
    println!("size_bytes:       {size}");
    // The log carries the recent writes while the bus is open, so the file alone under-reports it.
    let wal_size = {
        let mut path = db_path.as_os_str().to_os_string();
        path.push("-wal");
        std::fs::metadata(std::path::PathBuf::from(path))
            .map(|m| m.len())
            .unwrap_or(0)
    };
    println!("wal_bytes:        {wal_size}");
    println!("protocol_version: {version}");
    println!("created_by:       {created_by}");
    println!("topics:           {}", streams.len());
    println!("messages:         {messages}");
    println!("consumers:        {}", consumers.len());

    let presence_age = streams
        .iter()
        .find(|stream| stream.topic == presence::PRESENCE_ADDRESS)
        .map(|stream| (now - stream.oldest_at).max(0));
    match presence_age {
        Some(age) => println!("presence_age_s:   {age}"),
        None => println!("presence_age_s:   -"),
    }

    if !streams.is_empty() {
        // The third column is how long the oldest record has been sitting there: a stream that
        // never ages is the one nobody is reading.
        println!("\nstreams:          topic\trecords\tnewest\toldest_age_s");
        for stream in &streams {
            println!(
                "  {}\t{}\t{}\t{}",
                stream.topic,
                stream.records,
                stream.newest_offset,
                (now - stream.oldest_at).max(0)
            );
        }
    }
    if !consumers.is_empty() {
        // Lag is the distance between a cursor and the head of its stream: the one number that
        // says whether anything is stuck.
        println!("\nconsumers:        name\ttopic\tcursor\tlag");
        for (name, topic, offset) in &consumers {
            let head = streams
                .iter()
                .find(|stream| stream.topic == *topic)
                .map(|stream| stream.newest_offset)
                .unwrap_or(*offset);
            println!("  {name}\t{topic}\t{offset}\t{}", (head - offset).max(0));
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
    let (kind, body) = describe(&value);

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

/// The `event` object of a record, when that record is an acknowledgement.
fn ack_event(value: &Value) -> Option<&Value> {
    if value.get("type").and_then(Value::as_str) != Some("event") {
        return None;
    }
    let event = value.get("event")?;
    (event.get("kind").and_then(Value::as_str) == Some("ack")).then_some(event)
}

/// Human-facing kind and body for a record. An acknowledgement reads as a lifecycle
/// line instead of a JSON blob.
fn describe(value: &Value) -> (String, String) {
    if let Some(event) = ack_event(value) {
        let phase = event
            .get("phase")
            .and_then(Value::as_str)
            .unwrap_or("unknown");
        let detail = event
            .get("delivery")
            .or_else(|| event.get("outcome"))
            .and_then(Value::as_str);
        let mut body = match value.get("reply_to").and_then(Value::as_i64) {
            Some(reply_to) => format!("#{reply_to} {phase}"),
            None => phase.to_string(),
        };
        if let Some(detail) = detail {
            body.push(' ');
            body.push_str(detail);
        }
        return ("ack".to_string(), body);
    }

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
    (kind.to_string(), body)
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
