//! Command implementations.

use std::collections::{BTreeMap, BTreeSet};
use std::io::Read;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde_json::{Value, json};

use crate::address;
use crate::card;
use crate::config::resolve_retention;
use crate::error::Error;
use crate::keys;
use crate::presence::{self, Roster};
use crate::record;
use crate::store::{Bus, Store, StoredRecord};
use ed25519_dalek::SigningKey;

/// Advisory lock lifetime for a follow; renewed while running.
const LOCK_TTL_S: i64 = 60;
/// How often to renew follow locks.
const LOCK_RENEW_EVERY: Duration = Duration::from_secs(30);
/// How long a follow waits for updates before checking for an interrupt.
// A watchdog, not a poll: the follower listens for a notification and only looks again when one does
// not arrive. Kept well under the 30 s lock renewal so the renewal never starves.
const WAIT_SLICE: Duration = Duration::from_secs(10);
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
    /// Addresses to seal the body to, whatever the target address is.
    pub seal_to: Vec<String>,
    /// Store the body unsealed even when a recipient is known.
    pub plaintext: bool,
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
    /// Publish the question and return, without waiting for the answer.
    pub no_wait: bool,
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

/// Create this machine's signing key for a principal.
///
/// Deliberately takes no bus: making a key trusts nobody, so it needs no cooperation and cannot be
/// made to depend on one.
pub fn keygen(
    db: &crate::config::Target,
    principal: Option<String>,
    force: bool,
) -> Result<(), Error> {
    let principal = match principal {
        Some(name) => name,
        None => signing_principal_of(db)?,
    };
    let path = crate::keys::key_path(&principal)?;
    if path.exists() && !force {
        return Err(Error::usage(format!(
            "{} already exists; pass --force to replace it",
            path.display()
        )));
    }
    // A replaced key is archived, never deleted: a record sealed under it stays sealed under it
    // forever, so retiring a decryption key is deletion rather than policy.
    let archived = if path.exists() {
        let retired = crate::keys::archive_path(&principal, &today_utc())?;
        std::fs::rename(&path, &retired)
            .map_err(|e| Error::runtime(format!("cannot archive {}: {e}", path.display())))?;
        Some(retired)
    } else {
        None
    };

    let made = crate::keys::generate(&principal)?;
    println!("principal:   {principal}");
    println!("key:         {}", made.path.display());
    println!("fingerprint: {}", made.fingerprint);
    println!("public:      {}", made.public);
    if let Some(retired) = archived {
        println!("archived:    {}", retired.display());
        println!("             records sealed under it still open with it");
    }
    Ok(())
}

/// The principal this invocation signs as, asking the bus only when it knows.
///
/// A shared bus names the caller, and that name is what a peer pins — so a key made here belongs to
/// it. A local bus names nobody, and this machine names itself; asking it would open a file to learn
/// nothing.
fn signing_principal_of(db: &crate::config::Target) -> Result<String, Error> {
    if crate::config::credential().is_none() {
        return Ok(crate::keys::principal_name());
    }
    let (store, _) = Store::open(db)?;
    signing_principal(&store)
}

/// Print a principal's fingerprint, in the form a peer compares out of band.
pub fn fingerprint(db: &crate::config::Target, principal: Option<String>) -> Result<(), Error> {
    let principal = match principal {
        Some(name) => name,
        None => signing_principal_of(db)?,
    };
    let signing = crate::keys::load(&principal)?.ok_or_else(|| {
        Error::runtime(format!(
            "no key for {principal}; run 'msgq keygen {principal}' first"
        ))
    })?;
    println!("{}", crate::keys::fingerprint(&signing.verifying_key()));
    Ok(())
}

/// The principal this session signs as.
///
/// A bus that knows who the credential belongs to names it, because that is the name a peer pins. A
/// bus that does not — the local file — leaves the machine to name itself.
fn signing_principal(store: &Store) -> Result<String, Error> {
    Ok(match store.identity()? {
        Some((principal, _)) => principal,
        None => crate::keys::principal_name(),
    })
}

/// Sign a record with this session's key, making the key if this is the first record.
///
/// The notice goes to standard error: a publish prints an offset on standard output, and a script
/// parsing that offset should not have to know that a key was created.
fn sign_as_principal(value: &mut Value, store: &Store) -> Result<(), Error> {
    let principal = signing_principal(store)?;
    let (signing, made) = crate::keys::load_or_generate(&principal)?;
    if let Some(made) = made {
        eprintln!(
            "msgq: created a signing key for {principal} at {}",
            made.path.display()
        );
        eprintln!("msgq: fingerprint {}", made.fingerprint);
    }
    crate::keys::sign_value(value, &signing)
}

/// Seal the body by rule, and say which rule applied.
///
/// A record addressed to one participant is sealed for that participant and for its sender, so a
/// session can still read what it sent. A group is not a participant: it keeps the bus key and the
/// client says so, because a membership list is a surface this change does not invent.
fn seal_by_rule(
    value: &mut Value,
    args: &PublishArgs,
    store: &Store,
    canonical: &str,
    signing: &SigningKey,
) -> Result<(), Error> {
    if args.plaintext {
        // Worth one line, and only one: the caller asked for the unsafe thing explicitly.
        eprintln!("msgq: --plaintext given; the body is stored unsealed");
        return Ok(());
    }

    let sender = value
        .get("from")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();

    // A group keeps the bus key: the recipients of a group are not a list this bus can answer, and
    // inventing one is a change of its own.
    let explicit = !args.seal_to.is_empty();
    if !explicit && canonical.starts_with("session/") {
        return seal_with_bus_key(value);
    }

    let targets: Vec<String> = if explicit {
        args.seal_to.clone()
    } else {
        vec![canonical.to_string()]
    };

    let mut recipients = vec![crate::envelope::Recipient::this_machine(&sender, signing)];
    let mut unknown: Vec<String> = Vec::new();
    for target in &targets {
        match agreement_key_of(store, target)? {
            Some(agreement) => recipients.push(crate::envelope::Recipient {
                address: target.clone(),
                key: agreement,
            }),
            // An explicit recipient that cannot be found is the caller's mistake, and is refused.
            None if explicit => {
                return Err(Error::usage(format!(
                    "no agreement key known for {target}: a record from it would carry one, or pass --plaintext to store this unsealed"
                )));
            }
            // A first-contact publish is not a mistake: nobody has met yet. Fall back, and say so.
            None => unknown.push(target.clone()),
        }
    }

    if unknown.is_empty() {
        // Sealing to the recipient and the sender is the expected path, so it says nothing: a client
        // that narrates every ordinary publish is a client nobody reads.
        recipients.dedup_by(|a, b| a.key.as_bytes() == b.key.as_bytes());
        crate::envelope::seal_for(value, &recipients)?;
        return Ok(());
    }

    // Not sealed to the peer: the bus key, or plaintext. On a bus with no key material anywhere,
    // plaintext is the state of the world rather than a decision, and saying so every publish is noise.
    seal_with_bus_key(value)
}

/// Seal with the bus key when one is configured, and say what happened when one is.
/// Seal under the bus key when one is configured, silently.
///
/// No notice: an ordinary publish narrating which key it picked is noise on every line, and the
/// record itself declares what it was sealed with. `db info` reports the mode for anyone who wants to
/// know before they write, and a caller who asked for something a client cannot honour — a named
/// recipient with no key — gets an error instead.
fn seal_with_bus_key(value: &mut Value) -> Result<(), Error> {
    if let Some(key) = crate::seal::Key::from_env()? {
        crate::seal::seal_body(value, &key)?;
    }
    Ok(())
}

/// The agreement key of whoever writes at an address, found on the newest record they signed.
///
/// No directory and no lookup service: the key travels inside the bytes the signature covers, so any
/// record that address ever wrote answers this, including one written long before this session.
fn agreement_key_of(
    store: &Store,
    address: &str,
) -> Result<Option<x25519_dalek::PublicKey>, Error> {
    let wanted = address::canonicalize(address)?;

    // The roster is the cheap path: a live peer advertises its key on every presence renewal, and one
    // stream is a small read. A peer that is not live is the reason the key also rides on every record
    // it ever wrote, so the fallback is a bounded scan of what the bus still retains.
    let presence = presence::address(crate::config::scope().as_deref());
    if let Some(found) = newest_agreement_for(store, &presence, &wanted)? {
        return Ok(Some(found));
    }
    for topic in store.list_topics_names()? {
        if topic == presence {
            continue;
        }
        if let Some(found) = newest_agreement_for(store, &topic, &wanted)? {
            return Ok(Some(found));
        }
    }
    Ok(None)
}

/// The agreement key of `wanted`, from the newest record it signed in one stream.
fn newest_agreement_for(
    store: &Store,
    topic: &str,
    wanted: &str,
) -> Result<Option<x25519_dalek::PublicKey>, Error> {
    let head = store.max_offset(topic)?;
    if head == 0 {
        return Ok(None);
    }
    let window = 200i64;
    let events = store.read_since(topic, (head - window).max(0), window)?;
    for event in events.iter().rev() {
        let value: Value = match serde_json::from_slice(&event.payload) {
            Ok(value) => value,
            Err(_) => continue,
        };
        let Some(from) = value.get("from").and_then(Value::as_str) else {
            continue;
        };
        if address::canonicalize(from).ok().as_deref() != Some(wanted) {
            continue;
        }
        if let Some(agreement) = keys::claim(&value).and_then(|claim| claim.agreement) {
            return Ok(Some(agreement));
        }
    }
    Ok(None)
}

/// Publish one message to an address.
pub fn publish(db: &crate::config::Target, args: PublishArgs) -> Result<(), Error> {
    let canonical = address::canonicalize(&args.address)?;
    let mut value = build_record(&canonical, &args)?;
    let (store, _) = Store::open(db)?;

    // Sealing needs the store: a recipient's key is found on a record, not in a directory.
    let principal = signing_principal(&store)?;
    let signing = match crate::keys::load_or_generate(&principal)? {
        (signing, Some(made)) => {
            eprintln!(
                "msgq: created a signing key for {principal} at {}",
                made.path.display()
            );
            eprintln!("msgq: fingerprint {}", made.fingerprint);
            signing
        }
        (signing, None) => signing,
    };
    seal_by_rule(&mut value, &args, &store, &canonical, &signing)?;
    crate::keys::sign_value(&mut value, &signing)?;

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

/// Wait for acknowledgements of an already published record.
///
/// Acknowledgements are addressed to the sender of the original record, so the wait needs to know
/// which stream to watch. Offsets are unique only within a stream, so a bare offset names no
/// record: the caller gives a record id of the form address#offset, or an offset plus --on.
pub fn await_ack(
    db: &crate::config::Target,
    target: &str,
    on: Option<&str>,
    timeout_s: u64,
    json_out: bool,
) -> Result<(), Error> {
    let (store, _) = Store::open(db)?;
    let (stream, offset) = resolve_await_target(&store, target, on)?;
    // Re-attaching has no snapshot to take, so the reply stream is read from its start.
    await_terminal(&store, &stream, 0, offset, timeout_s, json_out)
}

/// Turn the await argument into the stream to watch and the offset being acknowledged.
///
/// Acknowledgements are addressed to the sender of the original record, which is not the stream the
/// original was published to. A record id therefore names the record being awaited and the wait
/// looks up who to watch; a bare offset cannot, because offsets are unique only within a stream, so
/// it needs --on to say where the acknowledgements will land.
fn resolve_await_target(
    store: &dyn Bus,
    target: &str,
    on: Option<&str>,
) -> Result<(String, i64), Error> {
    let trimmed = target.trim();
    if let Ok((address, offset)) = address::parse_record_id(trimmed) {
        if on.is_some() {
            return Err(Error::usage(format!(
                "await {trimmed} already names the record, so --on is not needed"
            )));
        }
        let events = store.read_since(&address, offset.saturating_sub(1), 1)?;
        let found = events.into_iter().find(|event| event.offset == offset);
        let Some(event) = found else {
            return Err(Error::usage(format!("no record at {trimmed}")));
        };
        let value: Value = serde_json::from_slice(&event.payload).unwrap_or(Value::Null);
        let from = value
            .get("from")
            .and_then(Value::as_str)
            .unwrap_or_default();
        if from.is_empty() {
            return Err(Error::usage(format!(
                "the record at {trimmed} names no sender, so the wait cannot be addressed"
            )));
        }
        return Ok((address::canonicalize(from)?, offset));
    }
    let Ok(offset) = trimmed.parse::<i64>() else {
        return Err(Error::usage(format!(
            "'{trimmed}' is not a record id; expected address#offset or an offset with --on"
        )));
    };
    match on {
        Some(on) => Ok((address::canonicalize(on)?, offset)),
        None => Err(Error::usage(format!(
            "offset {offset} alone does not name a record, because offsets are unique only within a stream; pass --on ADDRESS or a record id such as session/x#{offset}"
        ))),
    }
}

pub fn ask(db: &crate::config::Target, args: AskArgs) -> Result<(), Error> {
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
    let mut value = record::to_value(&rec)?;

    let (store, _) = Store::open(db)?;
    // The answer comes back to the sender, so record where that stream ends before asking.
    let stream = match args.on.as_deref() {
        Some(value) => address::canonicalize(value)?,
        None => address::canonicalize(&from)?,
    };
    let from_offset = store.max_offset(&stream)?;

    // A question is a record like any other: the peer that answers it must be able to tell who asked.
    sign_as_principal(&mut value, &store)?;
    let offset = store.publish(&canonical, &value)?;
    println!("{offset}");

    if args.no_wait {
        return Ok(());
    }

    let reader = crate::trust::Reader::new(db)?;
    await_reply(
        &store,
        &stream,
        from_offset,
        offset,
        args.timeout,
        args.json,
        &reader,
    )
}

/// Print the answer to an ask, or report why there is none yet.
fn await_reply(
    store: &dyn Bus,
    stream: &str,
    from_offset: i64,
    reply_to: i64,
    timeout_s: u64,
    json_out: bool,
    reader: &crate::trust::Reader,
) -> Result<(), Error> {
    let updates = store.wake(&[stream]);
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
            updates.wait(Duration::from_millis(500))?;
            continue;
        }

        for event in events {
            cursor = event.offset;
            let mut record: Value = serde_json::from_slice(&event.payload).unwrap_or(Value::Null);
            if record.get("reply_to").and_then(Value::as_i64) != Some(reply_to) {
                continue;
            }

            // The verdict is reached on the bytes that were signed, before anything opens them: a
            // sealed reply verified after decryption would read as a forgery of itself.
            let verdict = reader.verdict(&record);

            // A reply is sealed like any other record, and the asker is a recipient: it sealed to
            // itself when it asked, so what comes back is its to open.
            if crate::envelope::is_envelope(&record) {
                let opened = match crate::keys::load(&keys::principal_name())? {
                    Some(signing) => crate::envelope::open_for(&mut record, &signing),
                    None => Ok(crate::envelope::Opened::Elsewhere(Vec::new())),
                };
                if !matches!(opened, Ok(crate::envelope::Opened::Body)) {
                    continue;
                }
            } else if let Some(alg) = crate::envelope::declared_alg(&record) {
                // A sealed body that was not opened is never an answer. Echoing the stored bytes hands
                // the caller ciphertext dressed as content — the failure a mixed pair of clients
                // produces, and the one this message exists to make legible.
                eprintln!(
                    "msgq: the reply from {} is sealed with {alg}, which this build cannot open",
                    record
                        .get("from")
                        .and_then(Value::as_str)
                        .unwrap_or("somebody")
                );
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
                // The reply goes out in the one shape a stored record is printed in, verdict and all.
                println!(
                    "{}",
                    serde_json::to_string(&json!({
                        "topic": event.topic,
                        "offset": event.offset,
                        "created_at": event.created_at,
                        "verdict": verdict.to_json(),
                        "record": record,
                    }))?
                );
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

/// Print acknowledgements of `reply_to` until a terminal one arrives or the window closes.
fn await_terminal(
    store: &dyn Bus,
    stream: &str,
    from_offset: i64,
    reply_to: i64,
    timeout_s: u64,
    json_out: bool,
) -> Result<(), Error> {
    let updates = store.wake(&[stream]);
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
            updates.wait(Duration::from_millis(500))?;
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
    db: &crate::config::Target,
    addresses: &[String],
    from: FromSpec,
    limit: i64,
    json: bool,
    strict: bool,
) -> Result<(), Error> {
    let targets = address::canonicalize_all(addresses)?;
    let (store, _) = Store::open(db)?;
    let reader = crate::trust::Reader::new(db)?;
    report_conflicts(db)?;
    let limit = limit.max(1);

    let mut events: Vec<StoredRecord> = Vec::new();
    let mut lost = 0i64;
    let mut truncated: Vec<String> = Vec::new();
    for topic in &targets {
        let offset = match from {
            FromSpec::Start => 0,
            FromSpec::Offset(n) => n,
            FromSpec::End => store.max_offset(topic)?,
        };
        // A position below the oldest retained record has lost whatever retention removed beyond it.
        let oldest = store.oldest_offset(topic)?;
        let effective = match oldest {
            Some(first) if offset < first => {
                let dropped = store.retention_dropped(topic, offset)?;
                lost += dropped;
                truncated.push(topic.clone());
                first - 1
            }
            _ => offset,
        };
        events.extend(store.read_since(topic, effective, limit)?);
    }
    if lost > 0 {
        if strict {
            return Err(Error::runtime(format!(
                "{} record(s) are no longer retained in {}; refusing to print a partial view",
                lost,
                truncated.join(", ")
            )));
        }
        eprintln!(
            "msgq: {lost} record(s) are no longer retained in {}",
            truncated.join(", ")
        );
    }
    events.sort_by(|a, b| a.offset.cmp(&b.offset).then_with(|| a.topic.cmp(&b.topic)));
    events.truncate(limit as usize);

    // Verify before opening: a signature covers the bytes that are stored, so a sealed body is
    // checked as it sits on the bus, not as it reads once decrypted.
    let verdicts: Vec<crate::trust::Verdict> = events
        .iter()
        .map(|e| reader.verdict(&stored_value(e)))
        .collect();
    let failures = decrypt_events(&mut events, db)?;
    let mut refused: Vec<String> = Vec::new();
    let mut unsigned: Vec<String> = Vec::new();
    let mut strangers = 0usize;
    for (event, verdict) in events.iter().zip(&verdicts) {
        render(event, verdict, json);
        refuse(verdict, event, &mut refused, &mut unsigned);
        if matches!(
            verdict,
            crate::trust::Verdict::Unverified {
                fingerprint: Some(_),
                ..
            }
        ) {
            strangers += 1;
        }
    }
    if strangers > 0 {
        // Counted where the records are seen: a posture printed by another invocation could not know.
        eprintln!(
            "msgq: {strangers} record(s) came from senders with no anchor; 'msgq trust list' shows whom you trust"
        );
    }
    verdict_failures(&failures, &refused, &unsigned)
}

/// Refuse to print what the reader cannot trust, once every record has been shown with its verdict.
///
/// An unsigned record is only tolerable when the invocation asked for it, and a broken signature is
/// never tolerable. Both are reported after the output so a caller sees what arrived, then learns
/// that the read is not to be relied on.
fn verdict_failures(
    decrypt: &[String],
    refused: &[String],
    unsigned: &[String],
) -> Result<(), Error> {
    let mut problems: Vec<String> = Vec::new();
    if !decrypt.is_empty() {
        problems.push(format!(
            "cannot decrypt {} record(s): {}",
            decrypt.len(),
            decrypt.join("; ")
        ));
    }
    if !refused.is_empty() {
        problems.push(format!(
            "refused {} record(s): {}",
            refused.len(),
            refused.join("; ")
        ));
    }
    if !unsigned.is_empty() && !crate::trust::insecure() {
        problems.push(format!(
            "{} record(s) carry no usable signature (first: {}); set {}=1 to read them anyway",
            unsigned.len(),
            unsigned[0],
            crate::trust::INSECURE_ENV
        ));
    }
    if problems.is_empty() {
        return Ok(());
    }
    Err(Error::runtime(problems.join("; ")))
}

/// Note a verdict that a caller has to hear about.
fn refuse(
    verdict: &crate::trust::Verdict,
    event: &StoredRecord,
    refused: &mut Vec<String>,
    unsigned: &mut Vec<String>,
) {
    let id = address::record_id(&event.topic, event.offset);
    match verdict {
        crate::trust::Verdict::Refused { reason, .. } => {
            refused.push(format!("{id}: {reason}"));
        }
        // Nothing is exempt. Presence and acknowledgements are records like any other, and the client
        // signs them; a kind-based exemption would be a hole the moment somebody writes a record that
        // looks like progress.
        verdict if verdict.is_unsigned() => unsigned.push(id),
        _ => {}
    }
}

/// Name every principal whose trust layers disagree, before any record is shown.
///
/// A disagreement is reported and never reconciled: the precedence rule decides which anchor is
/// used, but only a person can decide which one is right.
fn report_conflicts(db: &crate::config::Target) -> Result<(), Error> {
    for conflict in crate::trust::conflicts(db)? {
        let claims: Vec<String> = conflict
            .claims
            .iter()
            .map(|(layer, fingerprint)| {
                format!("{} says {}", layer.name(), crate::trust::short(fingerprint))
            })
            .collect();
        eprintln!(
            "msgq: the trust layers disagree about {}: {}",
            conflict.principal,
            claims.join(", ")
        );
    }
    Ok(())
}

/// The sealing mode this invocation would use, for reporting.
fn sealing_mode(db: &crate::config::Target) -> Result<String, Error> {
    let principal = signing_principal_of(db)?;
    let Some(signing) = crate::keys::load(&principal)? else {
        return Ok(format!(
            "bus key only (no key for {principal} yet, so nothing can be sealed to it)"
        ));
    };
    let ours = crate::envelope::agreement_fingerprint(&crate::keys::agreement_public(&signing));
    Ok(match crate::seal::Key::from_env()? {
        Some(key) => format!(
            "recipients ({principal} at {}), bus key {} for groups",
            crate::trust::short(&ours),
            key.kid()
        ),
        None => format!("recipients ({principal} at {})", crate::trust::short(&ours)),
    })
}

/// Show what is trusted on this bus.
pub fn trust_list(db: &crate::config::Target, json: bool) -> Result<(), Error> {
    let listing = crate::trust::verbs::list(db)?;
    if json {
        println!(
            "{}",
            serde_json::to_string(&crate::trust::verbs::describe_json(
                &listing,
                crate::trust::insecure()
            ))?
        );
        return Ok(());
    }
    for line in crate::trust::verbs::describe(&listing, crate::trust::insecure()) {
        println!("{line}");
    }
    Ok(())
}

/// Who a trust anchor is for, and at which fingerprint.
///
/// With `--self` both come from this machine: the principal publish would sign as, and that key's
/// fingerprint. Trusting yourself needs no out-of-band channel — the key file is the claim and the
/// answer — so a fingerprint naming some other key is refused rather than recorded: a session whose
/// own records read unverified is a mistake dressed as a decision.
fn resolve_self(
    db: &crate::config::Target,
    principal: Option<&str>,
    fingerprint: Option<&str>,
    this_machine: bool,
) -> Result<(String, String), Error> {
    if !this_machine {
        let principal = principal.ok_or_else(|| {
            Error::usage("a trust anchor needs a principal: trust add <principal> <fingerprint>")
        })?;
        let fingerprint = fingerprint.ok_or_else(|| {
            Error::usage("a trust anchor needs a fingerprint: trust add <principal> <fingerprint>")
        })?;
        return Ok((principal.to_string(), fingerprint.to_string()));
    }

    // With --self there is no principal to name, so one positional is the fingerprint. Two would be
    // an attempt to pin somebody else through the back door.
    let named = match (principal, fingerprint) {
        (Some(_), Some(_)) => {
            return Err(Error::usage(
                "--self takes at most one fingerprint: trust add --self [<fingerprint>]",
            ));
        }
        (Some(one), None) | (None, Some(one)) => Some(one),
        (None, None) => None,
    };

    let principal = signing_principal_of(db)?;
    let key = crate::keys::load(&principal)?.ok_or_else(|| {
        Error::usage(format!(
            "no key for {principal} yet; publish once and it will be made, or name a fingerprint"
        ))
    })?;
    let own = crate::keys::fingerprint(&key.verifying_key());
    if let Some(given) = named {
        let given = crate::keys::normalize_fingerprint(given)?;
        if given != own {
            return Err(Error::usage(format!(
                "--self trusts the key this machine signs with, which is {own}; {given} is another key"
            )));
        }
    } else {
        eprintln!("msgq: this machine signs as {principal} at {own}");
    }
    Ok((principal, own))
}

/// Record that a principal is trusted at a fingerprint obtained out of band.
pub fn trust_add(
    db: &crate::config::Target,
    principal: Option<&str>,
    fingerprint: Option<&str>,
    this_machine: bool,
) -> Result<(), Error> {
    let (principal, fingerprint) = resolve_self(db, principal, fingerprint, this_machine)?;
    let recorded = crate::trust::verbs::add(db, &principal, &fingerprint)?;
    if recorded.unchanged {
        println!(
            "{} is already trusted at {}",
            recorded.anchor.principal, recorded.anchor.current
        );
        return Ok(());
    }
    println!(
        "trusted {} at {} on bus {}",
        recorded.anchor.principal,
        recorded.anchor.current,
        crate::trust::bus_id(db)?
    );
    println!("keyring: {}", recorded.path.display());
    Ok(())
}

/// Stop trusting a principal.
pub fn trust_revoke(db: &crate::config::Target, principal: &str) -> Result<(), Error> {
    let path = crate::trust::verbs::revoke(db, principal)?;
    println!(
        "revoked {} on bus {}",
        principal.trim(),
        crate::trust::bus_id(db)?
    );
    println!("keyring: {}", path.display());
    Ok(())
}

/// Replace a principal's key, and say how much history that stops being trusted.
pub fn trust_rotate(
    db: &crate::config::Target,
    principal: &str,
    fingerprint: &str,
) -> Result<(), Error> {
    let today = today_utc();
    let rotated = crate::trust::verbs::rotate(db, principal, fingerprint, &today)?;
    println!(
        "rotated {} from {} to {}",
        rotated.anchor.principal, rotated.previous, rotated.anchor.current
    );
    println!("keyring: {}", rotated.path.display());
    println!(
        "{} retained record(s) are now refused: they are signed by the retired key",
        rotated.refused_records
    );
    Ok(())
}

/// The date, without taking a dependency on the calendar for one line of output.
fn today_utc() -> String {
    let seconds = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let days = seconds / 86_400;
    // Civil-from-days, the same arithmetic a date library would do.
    let z = days as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!("{y:04}-{m:02}-{d:02}")
}

/// Open the bodies of records handed in on standard input, one outcome per record.
///
/// This exists so that a client which reads the bus itself obtains an opened body from one
/// implementation, exactly as it obtains a verdict from `verify`. Three outcomes, and only one of them
/// is a failure: opened, sealed to somebody else, or an encoding this build cannot honour.
pub fn open_records(db: &crate::config::Target, json: bool) -> Result<(), Error> {
    let principal = signing_principal_of(db)?;
    // The current key first, then everything it replaced: a record sealed under a retired key is
    // still that key's to open, and rotation must not lose it.
    let mut keys: Vec<SigningKey> = Vec::new();
    if let Some(current) = crate::keys::load(&principal)? {
        keys.push(current);
    }
    keys.extend(crate::keys::archived(&principal)?);
    let passphrase = crate::seal::Key::from_env()?;

    for line in read_stdin()?.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let outcome = open_one(line, &keys, passphrase.as_ref());
        if json {
            println!("{outcome}");
        } else {
            println!("{}", human_open(&outcome));
        }
    }
    Ok(())
}

/// Open one record, and say what happened in a form a caller can branch on.
fn open_one(line: &str, keys: &[SigningKey], passphrase: Option<&crate::seal::Key>) -> Value {
    let mut value: Value = match serde_json::from_str(line) {
        Ok(value @ Value::Object(_)) => value,
        _ => {
            return json!({
                "cannot_open": { "alg": "", "reason": "the line is not a record" }
            });
        }
    };

    if value.get("meta").and_then(|m| m.get("enc")).is_none() {
        // Nothing to open: a plaintext record is already what it says it is.
        return json!({ "opened": value });
    }

    if crate::envelope::is_envelope(&value) {
        if keys.is_empty() {
            return json!({
                "cannot_open": {
                    "alg": crate::envelope::ALG_ENVELOPE,
                    "reason": format!(
                        "no key for {}, so nothing sealed to it can be opened",
                        principal_hint()
                    )
                }
            });
        }
        // Every key this principal ever had, because the record may predate a rotation.
        let mut elsewhere: Vec<String> = Vec::new();
        let mut failure: Option<String> = None;
        for key in keys {
            let mut candidate = value.clone();
            match crate::envelope::open_for(&mut candidate, key) {
                Ok(crate::envelope::Opened::Body) => return json!({ "opened": candidate }),
                Ok(crate::envelope::Opened::Elsewhere(for_whom)) => elsewhere = for_whom,
                Ok(crate::envelope::Opened::UnknownAlgorithm(alg)) => {
                    return json!({
                        "cannot_open": { "alg": alg, "reason": "this build does not implement it" }
                    });
                }
                Err(error) => failure = Some(error.to_string()),
            }
        }
        return match failure {
            Some(reason) => json!({
                "cannot_open": { "alg": crate::envelope::ALG_ENVELOPE, "reason": reason }
            }),
            None => json!({ "sealed_to": elsewhere }),
        };
    }

    // The passphrase scheme, unchanged: old records must open exactly as they did.
    match passphrase {
        Some(key) => match crate::seal::open_body(&mut value, key) {
            Ok(()) => json!({ "opened": value }),
            Err(error) => {
                json!({ "cannot_open": { "alg": crate::seal::ALG, "reason": error.to_string() } })
            }
        },
        None => json!({
            "cannot_open": {
                "alg": crate::seal::ALG,
                "reason": format!("no passphrase is configured; set {}", crate::seal::PASSPHRASE_ENV)
            }
        }),
    }
}

fn principal_hint() -> String {
    crate::keys::principal_name()
}

/// The same outcome as one line, for a person.
fn human_open(outcome: &Value) -> String {
    if let Some(record) = outcome.get("opened") {
        let body = record
            .get("message")
            .and_then(Value::as_str)
            .map(str::to_string)
            .unwrap_or_else(|| {
                record
                    .get("event")
                    .map(|event| event.to_string())
                    .unwrap_or_default()
            });
        return body;
    }
    if let Some(sealed_to) = outcome.get("sealed_to") {
        let who: Vec<String> = sealed_to
            .as_array()
            .map(|list| {
                list.iter()
                    .filter_map(Value::as_str)
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_default();
        return format!("sealed to {}", who.join(", "));
    }
    let alg = outcome
        .get("cannot_open")
        .and_then(|c| c.get("alg"))
        .and_then(Value::as_str)
        .unwrap_or("");
    let reason = outcome
        .get("cannot_open")
        .and_then(|c| c.get("reason"))
        .and_then(Value::as_str)
        .unwrap_or("");
    format!("cannot open ({alg}): {reason}")
}

/// Verify records handed in on standard input, one verdict per record.
///
/// This exists so that a client which reads the bus itself — the pi extension does — obtains the same
/// verdict as the command-line reader, from one implementation, without a second copy of the crypto.
pub fn verify(db: &crate::config::Target, json: bool) -> Result<(), Error> {
    let reader = crate::trust::Reader::new(db)?;
    for line in read_stdin()?.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let verdict = match serde_json::from_str::<Value>(line) {
            Ok(value) if value.is_object() => reader.verdict(&value),
            _ => crate::trust::Verdict::Unverified {
                fingerprint: None,
                reason: "the line is not a record".to_string(),
            },
        };
        if json {
            println!("{}", verdict.to_json());
        } else {
            println!("{}", verdict.line());
        }
    }
    Ok(())
}

/// Open every sealed body in a batch, in place.
///
/// Returns one line per record that could not be opened. A body that does not open is removed rather
/// than printed: an unopenable body is an error, never content.
fn decrypt_events(
    events: &mut [StoredRecord],
    db: &crate::config::Target,
) -> Result<Vec<String>, Error> {
    let key = crate::seal::Key::from_env()?;
    let mut failures = Vec::new();
    for event in events.iter_mut() {
        let mut value: Value = serde_json::from_slice(&event.payload).unwrap_or(Value::Null);
        if crate::seal::enc_info(&value).is_none() {
            continue;
        }

        // An envelope addressed to recipients is opened when this session is one of them. When it is
        // not, the body is left exactly as stored: it is somebody else's record, not a failure, and
        // it has to survive the read so that whoever holds the key can open it through `msgq open`.
        if crate::envelope::is_envelope(&value) {
            // The principal this invocation signs as, so a shared bus opens what its identity was
            // sealed for rather than what this machine's shell name would have been.
            let Some(signing) = crate::keys::load(&signing_principal_of(db)?)? else {
                continue;
            };
            match crate::envelope::open_for(&mut value, &signing) {
                Ok(crate::envelope::Opened::Body) => event.payload = serde_json::to_vec(&value)?,
                Ok(crate::envelope::Opened::Elsewhere(_))
                | Ok(crate::envelope::Opened::UnknownAlgorithm(_)) => {}
                Err(reason) => {
                    crate::seal::blank_body(&mut value);
                    event.payload = serde_json::to_vec(&value)?;
                    failures.push(format!("{}#{}: {reason}", event.topic, event.offset));
                }
            }
            continue;
        }

        let outcome = match &key {
            Some(key) => crate::seal::open_body(&mut value, key).map_err(|error| error.to_string()),
            None => Err(
                "no passphrase is configured; set MSGQ_PASSPHRASE or MSGQ_PASSPHRASE_FILE"
                    .to_string(),
            ),
        };
        match outcome {
            Ok(()) => event.payload = serde_json::to_vec(&value)?,
            Err(reason) => {
                crate::seal::blank_body(&mut value);
                event.payload = serde_json::to_vec(&value)?;
                failures.push(format!("{}#{}: {reason}", event.topic, event.offset));
            }
        }
    }
    Ok(failures)
}

/// Take one lock per followed address, so two processes cannot share a consumer's cursor.
fn take_follow_locks(
    store: &dyn Bus,
    targets: &[String],
    consumer: &str,
    owner: &str,
) -> Result<Vec<Box<dyn crate::store::FollowLock>>, Error> {
    let mut locks: Vec<Box<dyn crate::store::FollowLock>> = Vec::new();
    for topic in targets {
        let name = format!("msgq:follow:{consumer}:{topic}");
        let Some(lock) = store.try_lock(&name, owner, LOCK_TTL_S)? else {
            return Err(Error::runtime(format!(
                "consumer '{consumer}' is already following '{topic}' in another process"
            )));
        };
        locks.push(lock);
    }
    Ok(locks)
}

/// Where each followed stream starts, after reporting anything retention already took.
///
/// A follow never stops on a gap: report the loss and carry on from the oldest retained record, so a
/// session that was offline keeps receiving.
fn starting_positions(
    store: &dyn Bus,
    targets: &[String],
    consumer: &str,
    from: FromSpec,
) -> Result<BTreeMap<String, i64>, Error> {
    let mut offsets = BTreeMap::new();
    for topic in targets {
        let start = match from {
            FromSpec::Start => 0,
            FromSpec::Offset(n) => n,
            FromSpec::End => store
                .cursor_get(consumer, topic)?
                .unwrap_or(store.head(topic)?),
        };
        let resumed = match store.oldest_offset(topic)? {
            Some(first) if start < first => {
                eprintln!(
                    "msgq: {} record(s) are no longer retained in {topic}",
                    store.retention_dropped(topic, start)?
                );
                first - 1
            }
            _ => start,
        };
        offsets.insert(topic.clone(), resumed);
    }
    Ok(offsets)
}

/// Order one round of records by stream position, so a follow is deterministic across addresses.
fn sort_by_position(batch: &mut [StoredRecord]) {
    batch.sort_by(|a, b| a.offset.cmp(&b.offset).then_with(|| a.topic.cmp(&b.topic)));
}

/// Renew every follow lock; best effort, because losing one only risks a second follower.
fn renew_all(locks: &[Box<dyn crate::store::FollowLock>]) {
    for lock in locks {
        let _ = lock.heartbeat(LOCK_TTL_S);
    }
}

/// Cursor-based follow: resumes a consumer and advances its offsets.
pub fn follow(
    db: &crate::config::Target,
    addresses: &[String],
    consumer: &str,
    from: FromSpec,
    json: bool,
) -> Result<(), Error> {
    let targets = address::canonicalize_all(addresses)?;
    let (store, _) = Store::open(db)?;
    let reader = crate::trust::Reader::new(db)?;
    let owner = format!("msgq:{}:{}", std::process::id(), nanos());

    let locks = take_follow_locks(&store, &targets, consumer, &owner)?;
    let mut offsets = starting_positions(&store, &targets, consumer, from)?;

    let interrupted = Arc::new(AtomicBool::new(false));
    {
        let flag = Arc::clone(&interrupted);
        let _ = ctrlc::set_handler(move || flag.store(true, Ordering::SeqCst));
    }

    let watched: Vec<&str> = targets.iter().map(String::as_str).collect();
    let updates = store.wake(&watched);
    let mut last_renew = Instant::now();

    while !interrupted.load(Ordering::SeqCst) {
        let mut batch: Vec<StoredRecord> = Vec::new();
        for topic in &targets {
            let offset = *offsets.get(topic).unwrap_or(&0);
            batch.extend(store.read_since(topic, offset, FOLLOW_BATCH)?);
        }
        sort_by_position(&mut batch);

        if batch.is_empty() {
            if last_renew.elapsed() >= LOCK_RENEW_EVERY {
                renew_all(&locks);
                last_renew = Instant::now();
            }
            updates.wait(WAIT_SLICE)?;
            continue;
        }

        for failure in decrypt_events(&mut batch, db)? {
            eprintln!("msgq: {failure}");
        }
        for event in batch {
            store.cursor_set(consumer, &event.topic, event.offset)?;
            offsets.insert(event.topic.clone(), event.offset);
            let verdict = reader.verdict(&stored_value(&event));
            render(&event, &verdict, json);
            let mut refused = Vec::new();
            let mut unsigned = Vec::new();
            refuse(&verdict, &event, &mut refused, &mut unsigned);
            verdict_failures(&[], &refused, &unsigned)?;
        }
    }

    Ok(())
}

/// List distinct topics with message counts and highest offsets.
pub fn topics(
    db: &crate::config::Target,
    json_out: bool,
    names: bool,
    card: bool,
) -> Result<(), Error> {
    let (store, _) = Store::open(db)?;
    let rows = store.list_topics()?;
    if card {
        // The roster answers "who is this" for the addresses a participant advertises; the rest keep
        // the same two columns with `-`, so a picker never has to special-case a row.
        let cards = cards_by_address(&store)?;
        for topic in store.list_topics_names()? {
            let description = cards
                .get(&topic)
                .map(card::summary_compact)
                .unwrap_or_else(|| "-".to_string());
            println!("{topic}\t{description}");
        }
        return Ok(());
    }
    if names {
        // No count is printed, so none is asked for.
        for topic in store.list_topics_names()? {
            println!("{topic}");
        }
        return Ok(());
    }
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
    db: &crate::config::Target,
    topic: Option<&str>,
    action: ConsumersAction,
    json_out: bool,
    names: bool,
) -> Result<(), Error> {
    let topic = match topic {
        Some(t) => Some(address::canonicalize(t)?),
        None => None,
    };
    let (store, _) = Store::open(db)?;

    match action {
        ConsumersAction::List => {
            let rows = store.list_consumers(topic.as_deref())?;
            let mut described: Vec<(String, String, i64, i64, i64)> = Vec::new();
            for (name, topic, offset) in rows {
                // Lag is how far behind the newest record the cursor sits; dropped is how much
                // retention already took out from under it.
                let lag = (store.max_offset(&topic)? - offset).max(0);
                let dropped = store.retention_dropped(&topic, offset)?;
                described.push((name, topic, offset, lag, dropped));
            }
            if names {
                for (name, _, _, _, _) in &described {
                    println!("{name}");
                }
                return Ok(());
            }
            if json_out {
                let array: Vec<Value> = described
                    .iter()
                    .map(|(name, topic, offset, lag, dropped)| {
                        json!({"consumer": name, "topic": topic, "offset": offset,
                               "lag": lag, "dropped": dropped})
                    })
                    .collect();
                println!("{}", serde_json::to_string(&array)?);
            } else {
                if let Some((owner, _scopes)) = store.identity()? {
                    // Cursors belong to the principal that wrote them, so the listing names it.
                    println!("owner:\t{owner}");
                }
                for (name, topic, offset, lag, dropped) in described {
                    println!("{name}\t{topic}\t{offset}\t{lag}\t{dropped}");
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
/// The card of the participant advertising each address, from this scope's roster.
///
/// One read, shared by every listing that wants to say who an address is.
fn cards_by_address(store: &dyn Bus) -> Result<BTreeMap<String, Value>, Error> {
    let now = unix_now();
    let mut newest: BTreeMap<String, (i64, Value)> = BTreeMap::new();
    for (created_at, record) in presence_tail(store)? {
        if presence::is_retraction(&record.state) || created_at + record.ttl <= now {
            continue;
        }
        let Some(card) = record.card.clone() else {
            continue;
        };
        let fresher = newest
            .get(&record.address)
            .is_none_or(|(best, _)| created_at > *best);
        if fresher {
            newest.insert(record.address.clone(), (created_at, card));
        }
    }
    Ok(newest
        .into_iter()
        .map(|(address, (_, card))| (address, card))
        .collect())
}

fn presence_tail(store: &dyn Bus) -> Result<Vec<(i64, presence::PresenceRecord)>, Error> {
    let presence_address = presence::address(crate::config::scope().as_deref());
    let head = store.head(&presence_address)?;
    let start = (head - presence::SCAN_RECORDS).max(0);
    let events = store.read_since(&presence_address, start, presence::SCAN_RECORDS)?;

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
fn live_participants(store: &dyn Bus) -> Result<BTreeSet<String>, Error> {
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
///
/// Text columns are address, kind, state, alias and remaining lifetime; `--wide` adds one locator
/// column built by [`card::summary`]. `--json` carries the card as an object, unchanged.
pub fn sessions(
    db: &crate::config::Target,
    json_out: bool,
    wide: bool,
    names: bool,
    card: bool,
) -> Result<(), Error> {
    let (store, _) = Store::open(db)?;
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
                card: record.card.clone(),
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

    if card {
        for entry in &entries {
            let description = entry
                .card
                .as_ref()
                .map(card::summary_compact)
                .unwrap_or_else(|| "-".to_string());
            println!("{}\t{description}", entry.address);
        }
        return Ok(());
    }
    if names {
        for entry in &entries {
            println!("{}", entry.address);
        }
        return Ok(());
    }
    if json_out {
        for entry in &entries {
            let mut value = json!({
                "address": entry.address,
                "kind": entry.kind,
                "state": entry.state,
                "alias": entry.alias,
                "expires_at": entry.expires_at,
                "expires_in": (entry.expires_at - now).max(0),
            });
            if let Some(card) = &entry.card {
                value["card"] = card.clone();
            }
            println!("{}", serde_json::to_string(&value)?);
        }
    } else {
        for entry in &entries {
            let mut line = format!(
                "{}\t{}\t{}\t{}\t{}",
                entry.address,
                entry.kind,
                entry.state,
                entry.alias.clone().unwrap_or_default(),
                (entry.expires_at - now).max(0)
            );
            if wide {
                let locator = entry
                    .card
                    .as_ref()
                    .map(card::summary)
                    .unwrap_or_else(|| "-".to_string());
                line.push('\t');
                line.push_str(&locator);
            }
            println!("{line}");
        }
    }
    Ok(())
}

/// Parse an age such as `7d`, `36h`, `90m` or `45s` into seconds.
/// What `msgq prune` was asked to do.
pub struct PruneArgs {
    /// From --older-than or --retention; None uses the configured window.
    pub window: Option<String>,
    pub apply: bool,
}

/// Records removed per statement, so a large prefix is never one unbounded write.
const PRUNE_BATCH: usize = 500;

/// Remove records older than an age, never removing one a consumer has not passed.
pub fn prune(db: &crate::config::Target, args: PruneArgs) -> Result<(), Error> {
    let age = resolve_retention(args.window.as_deref())?;
    let (store, _) = Store::open(db)?;
    let threshold = unix_now() - age;

    // Presence is a window, not a history: the newest record of every participant is what the
    // roster reads, so it stays even when it is older than the window.
    let presence_address = presence::address(crate::config::scope().as_deref());
    let presence_keep = store.newest_presence_offsets(&presence_address)?;

    // A cursor pins nothing. Keeping a record because one consumer has not passed it would let a
    // single offline session hold history forever, which is the opposite of a short window; the
    // session is named in the report and told what it lost instead.
    let mut losing: BTreeSet<String> = BTreeSet::new();

    let mut planned = 0i64;
    let mut removed = 0i64;
    let mut batches = 0usize;
    let mut report: Vec<String> = Vec::new();

    for stream in store.stream_summaries()? {
        let Some(end) = store.prefix_end(&stream.topic, threshold)? else {
            continue;
        };
        let keep: &[i64] = if stream.topic == presence_address {
            &presence_keep
        } else {
            &[]
        };
        let count = store.count_stream_prefix(&stream.topic, end, keep)?;
        if count <= 0 {
            continue;
        }
        planned += count;
        for (name, _topic, offset) in store.list_consumers(Some(&stream.topic))? {
            if offset < end {
                losing.insert(name);
            }
        }
        if args.apply {
            let removal = store.delete_stream_prefix(&stream.topic, end, keep, PRUNE_BATCH)?;
            removed += removal.removed as i64;
            batches += removal.batches;
            // The engine cannot count what it deleted after the fact, so the watermark carries the
            // running total: that is what a lagging consumer reads to learn what it lost.
            let previous = store
                .retention_watermark(&stream.topic)?
                .map(|(_, total)| total)
                .unwrap_or(0);
            store.retention_record(&stream.topic, end, previous + removal.removed as i64)?;
        }
        report.push(format!(
            "  {}\t{} of {}",
            stream.topic, count, stream.records
        ));
    }

    if report.is_empty() {
        println!("nothing is older than the retention window ({age}s)");
        return Ok(());
    }
    for line in &report {
        println!("{line}");
    }
    if !losing.is_empty() {
        let names: Vec<&str> = losing.iter().take(5).map(String::as_str).collect();
        let more = losing.len().saturating_sub(names.len());
        let tail = if more > 0 {
            format!(" and {more} more")
        } else {
            String::new()
        };
        eprintln!(
            "msgq: {} consumer(s) will lose records: {}{tail}",
            losing.len(),
            names.join(", ")
        );
    }
    if !presence_keep.is_empty() {
        eprintln!(
            "msgq: the newest presence record of {} participant(s) is kept",
            presence_keep.len()
        );
    }
    if args.apply {
        println!("removed {removed} record(s) in {batches} batch(es)");
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
pub fn consumers_gc(db: &crate::config::Target, apply: bool) -> Result<(), Error> {
    let (store, _) = Store::open(db)?;
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
pub fn cursor(db: &crate::config::Target, action: CursorAction) -> Result<(), Error> {
    let (store, _) = Store::open(db)?;
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
pub fn db_info(db: &crate::config::Target) -> Result<(), Error> {
    let local = db.local_path().map(|path| path.to_path_buf());
    let existed_before = local.as_ref().is_some_and(|path| path.exists());
    let (store, created) = Store::open(db)?;
    let now = unix_now();

    let size = local
        .as_ref()
        .and_then(|path| std::fs::metadata(path).ok())
        .map(|m| m.len())
        .unwrap_or(0);
    let version = store
        .meta("protocol_version")?
        .unwrap_or_else(|| "unknown".to_string());
    let created_by = store
        .meta("created_by")?
        .unwrap_or_else(|| "unknown".to_string());
    let streams = store.stream_summaries()?;
    let consumers = store.list_consumers(None)?;
    let messages: i64 = streams.iter().map(|stream| stream.records).sum();

    println!("path:             {}", db.display());
    println!(
        "existed_before:   {}",
        if existed_before { "yes" } else { "no" }
    );
    println!("created:          {}", if created { "yes" } else { "no" });
    println!("size_bytes:       {size}");
    // The log carries the recent writes while the bus is open, so the file alone under-reports it.
    let wal_size = local
        .as_ref()
        .map(|path| {
            let mut path = path.as_os_str().to_os_string();
            path.push("-wal");
            std::fs::metadata(std::path::PathBuf::from(path))
                .map(|m| m.len())
                .unwrap_or(0)
        })
        .unwrap_or(0);
    println!("wal_bytes:        {wal_size}");
    println!("protocol_version: {version}");
    println!("created_by:       {created_by}");
    println!("topics:           {}", streams.len());
    println!("messages:         {messages}");
    println!("consumers:        {}", consumers.len());
    println!("backend:          {}", store.engine());
    println!(
        "retention_s:      {}",
        resolve_retention(None).unwrap_or(crate::config::DEFAULT_RETENTION_S)
    );
    println!("credential:       {}", store.credential()?);
    println!("trust_bus:        {}", crate::trust::bus_id(db)?);
    println!(
        "trust_unsigned:   {}",
        if crate::trust::insecure() {
            "read (MSGQ_INSECURE is set)"
        } else {
            "refused"
        }
    );
    let listing = crate::trust::verbs::list(db)?;
    let layers: Vec<String> = listing
        .in_force
        .iter()
        .map(|l| l.layer.name().to_string())
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect();
    println!(
        "trust_anchors:    {} in force{}",
        listing.in_force.len(),
        if layers.is_empty() {
            String::new()
        } else {
            format!(" ({})", layers.join(", "))
        }
    );
    for conflict in &listing.conflicts {
        let claims: Vec<String> = conflict
            .claims
            .iter()
            .map(|(layer, fingerprint)| {
                format!("{} says {}", layer.name(), crate::trust::short(fingerprint))
            })
            .collect();
        println!(
            "trust_conflict:   the layers disagree about {}: {}",
            conflict.principal,
            claims.join(", ")
        );
    }
    println!("encryption:       {}", crate::seal::describe()?);
    println!("sealing:          {}", sealing_mode(db)?);
    if let Some((principal, scopes)) = store.identity()? {
        println!(
            "identity:         {principal} (scopes: {})",
            scopes.join(", ")
        );
    }

    let presence_age = streams
        .iter()
        .find(|stream| stream.topic == presence::address(crate::config::scope().as_deref()))
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
/// Print one stored record with its position and its verdict, and hand the verdict back.
///
/// There is one shape, in both output modes: the record stays exactly the bytes the bus stored, and
/// everything a reader adds — where it sits and what it is worth — lives beside it, never inside it.
fn render(event: &StoredRecord, verdict: &crate::trust::Verdict, json_out: bool) {
    let value: Value = serde_json::from_slice(&event.payload).unwrap_or(Value::Null);

    if json_out {
        println!(
            "{}",
            serde_json::to_string(&json!({
                "topic": event.topic,
                "offset": event.offset,
                "created_at": event.created_at,
                "verdict": verdict.to_json(),
                "record": value,
            }))
            .unwrap_or_else(|_| String::from_utf8_lossy(&event.payload).into_owned())
        );
        return;
    }

    let from = value.get("from").and_then(Value::as_str).unwrap_or("?");
    let (kind, body) = describe(&value);
    println!(
        "{:>7}  {}  {:<24}  {:<9}  {:<14}  {:<20}  {}",
        event.offset,
        format_unix(event.created_at),
        event.topic,
        kind,
        verdict.state(),
        from,
        body
    );
}

/// The record a stored event carries, as the bus holds it.
fn stored_value(event: &StoredRecord) -> Value {
    serde_json::from_slice(&event.payload).unwrap_or(Value::Null)
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
    // A body this build cannot interpret is an error, not content: printing ciphertext as if it
    // were a message would be worse than saying so.
    if let Some(kid) = record::opaque_body(value) {
        return (kind.to_string(), format!("cannot decrypt: kid={kid}"));
    }
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
