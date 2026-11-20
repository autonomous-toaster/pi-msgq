//! The trust surface end to end: one shape for a record, one verdict on every path, and a strict
//! default that only an explicit permission relaxes.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::Value;

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_msgq")
}

fn temp_dir() -> PathBuf {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);

    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let unique = COUNTER.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("msgq-tr-{}-{nanos}-{unique}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

struct Run {
    code: i32,
    stdout: String,
    stderr: String,
}

fn run(home: &Path, db: &Path, args: &[&str], env: &[(&str, &str)]) -> Run {
    let mut command = Command::new(bin());
    command
        .env("HOME", home)
        .env_remove("MSGQ_TRUST")
        .env_remove("MSGQ_TRUST_SESSION")
        .env_remove("MSGQ_INSECURE")
        .env_remove("MSGQ_PASSPHRASE")
        .env_remove("MSGQ_TOKEN")
        .arg("--db")
        .arg(db)
        .args(args);
    for (key, value) in env {
        command.env(key, value);
    }
    let out = command.output().unwrap();
    Run {
        code: out.status.code().unwrap_or(-1),
        stdout: String::from_utf8_lossy(&out.stdout).to_string(),
        stderr: String::from_utf8_lossy(&out.stderr).to_string(),
    }
}

fn wrapped(line: &str) -> Value {
    serde_json::from_str(line).unwrap_or_else(|e| panic!("not a JSON line: {line}: {e}"))
}

#[test]
fn a_record_is_wrapped_with_its_position_and_its_verdict() {
    let home = temp_dir();
    let db = home.join("bus.db");
    assert_eq!(run(&home, &db, &["publish", "group/a", "hi"], &[]).code, 0);

    let out = run(
        &home,
        &db,
        &["read", "group/a", "--from-start", "--json"],
        &[],
    );
    let value = wrapped(out.stdout.trim());
    for key in ["topic", "offset", "created_at", "verdict", "record"] {
        assert!(value.get(key).is_some(), "the shape has no {key}: {value}");
    }
    assert_eq!(value["record"]["message"], "hi");
    // The record is the wire record: nothing a reader adds may appear inside it.
    assert!(value["record"].get("verdict").is_none());
    assert!(value["record"].get("offset").is_none());
    assert_eq!(value["verdict"]["state"], "unverified");
}

#[test]
fn a_pinned_key_makes_a_record_trusted() {
    let home = temp_dir();
    let db = home.join("bus.db");
    assert_eq!(run(&home, &db, &["publish", "group/a", "hi"], &[]).code, 0);
    let fingerprint = run(&home, &db, &["fingerprint"], &[])
        .stdout
        .trim()
        .to_string();

    let trust = format!("alice:{fingerprint}");
    let out = run(
        &home,
        &db,
        &["read", "group/a", "--from-start", "--json"],
        &[("MSGQ_TRUST", &trust)],
    );
    let value = wrapped(out.stdout.trim());
    assert_eq!(value["verdict"]["state"], "trusted", "{value}");
    assert_eq!(value["verdict"]["principal"], "alice");
    // A trusted record is an ordinary record: nothing new to complain about.
    assert_eq!(out.code, 0, "{}", out.stderr);
}

#[test]
fn a_broken_signature_is_refused_and_names_what_it_claims() {
    let home = temp_dir();
    let db = home.join("bus.db");

    // The fixture is signed by a key whose body was then altered. Writing it straight onto the bus
    // is what a foreign writer does — this client would have refused to sign it — and the reader is
    // the one who objects.
    let here = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../protocol/fixtures");
    let tampered: Value = serde_json::from_str(
        &std::fs::read_to_string(here.join("19-signature-tampered.json")).unwrap(),
    )
    .unwrap();
    let target = msgq::config::Target::Local(db.clone());
    let (store, _) = msgq::store::Store::open(&target).unwrap();
    msgq::store::Bus::publish(&store, "group/red", &tampered).unwrap();

    let out = run(
        &home,
        &db,
        &["read", "group/red", "--from-start", "--json"],
        &[],
    );
    let value = wrapped(out.stdout.trim());
    let verdict = &value["verdict"];
    assert_eq!(verdict["state"], "refused", "{value}");
    assert!(
        verdict["reason"]
            .as_str()
            .unwrap()
            .contains("does not cover"),
        "{verdict}"
    );
    assert!(
        verdict["fingerprint"]
            .as_str()
            .unwrap()
            .starts_with("SHA256:")
    );
    assert_eq!(out.code, 1, "a refused record is not a successful read");
    assert!(out.stderr.contains("refused"), "{}", out.stderr);
}

#[test]
fn an_unsigned_message_is_refused_until_the_invocation_permits_it() {
    let home = temp_dir();
    let db = home.join("bus.db");

    // Written as another client would: straight onto the bus, with no signature at all.
    let target = msgq::config::Target::Local(db.clone());
    let (store, _) = msgq::store::Store::open(&target).unwrap();
    msgq::store::Bus::publish(
        &store,
        "group/a",
        &serde_json::json!({
            "v": 1, "type": "note", "from": "cli/foreign", "to": "group/a", "message": "legacy"
        }),
    )
    .unwrap();

    let refused = run(
        &home,
        &db,
        &["read", "group/a", "--from-start", "--json"],
        &[],
    );
    assert_eq!(refused.code, 1);
    assert!(
        refused.stderr.contains("no usable signature"),
        "{}",
        refused.stderr
    );
    assert!(
        refused.stderr.contains("MSGQ_INSECURE"),
        "{}",
        refused.stderr
    );

    let allowed = run(
        &home,
        &db,
        &["read", "group/a", "--from-start", "--json"],
        &[("MSGQ_INSECURE", "1")],
    );
    assert_eq!(allowed.code, 0, "{}", allowed.stderr);
    let value = wrapped(allowed.stdout.trim());
    assert_eq!(value["verdict"]["state"], "unverified");
    assert_eq!(value["verdict"]["reason"], "no usable signature");
}

#[test]
fn bookkeeping_records_are_not_refused() {
    let home = temp_dir();
    let db = home.join("bus.db");

    // Presence and acknowledgements are public metadata about a participant, and no client signs
    // them. A strict default that refused them would refuse the bus's own progress.
    let target = msgq::config::Target::Local(db.clone());
    let (store, _) = msgq::store::Store::open(&target).unwrap();
    msgq::store::Bus::publish(
        &store,
        "group/roster",
        &serde_json::json!({
            "v": 1, "type": "event", "from": "session/a", "to": "group/roster",
            "event": { "kind": "presence", "state": "idle", "ttl_s": 60 }
        }),
    )
    .unwrap();

    let out = run(
        &home,
        &db,
        &["read", "group/roster", "--from-start", "--json"],
        &[],
    );
    assert_eq!(out.code, 0, "{}", out.stderr);
    assert_eq!(wrapped(out.stdout.trim())["verdict"]["state"], "unverified");
}

#[test]
fn verify_reports_one_verdict_per_record_in_order() {
    let home = temp_dir();
    let db = home.join("bus.db");
    assert_eq!(run(&home, &db, &["publish", "group/a", "hi"], &[]).code, 0);
    let fingerprint = run(&home, &db, &["fingerprint"], &[])
        .stdout
        .trim()
        .to_string();

    let signed = run(
        &home,
        &db,
        &["read", "group/a", "--from-start", "--json"],
        &[],
    );
    let record = wrapped(signed.stdout.trim())["record"].clone();
    let unsigned = serde_json::json!({
        "v": 1, "type": "note", "from": "cli/foreign", "to": "group/a", "message": "legacy"
    });

    let input = format!(
        "{}
{}
{}
",
        serde_json::to_string(&record).unwrap(),
        serde_json::to_string(&unsigned).unwrap(),
        "not a record"
    );

    let trust = format!("alice:{fingerprint}");
    let mut child = Command::new(bin())
        .env("HOME", &home)
        .env_remove("MSGQ_PASSPHRASE")
        .env("MSGQ_TRUST", &trust)
        .arg("--db")
        .arg(&db)
        .args(["verify", "--json"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    {
        use std::io::Write;
        child
            .stdin
            .as_mut()
            .unwrap()
            .write_all(input.as_bytes())
            .unwrap();
    }
    let out = child.wait_with_output().unwrap();
    assert_eq!(out.status.code(), Some(0));
    let text = String::from_utf8_lossy(&out.stdout).to_string();
    let states: Vec<String> = text
        .trim()
        .lines()
        .map(|l| wrapped(l)["state"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(
        states,
        vec!["trusted", "unverified", "unverified"],
        "{text}"
    );

    // The separate path agrees with the read path about the same bytes.
    let read = run(
        &home,
        &db,
        &["read", "group/a", "--from-start", "--json"],
        &[("MSGQ_TRUST", &trust)],
    );
    assert_eq!(wrapped(read.stdout.trim())["verdict"]["state"], "trusted");
}

#[test]
fn verify_explains_an_unreadable_line_rather_than_failing() {
    let home = temp_dir();
    let db = home.join("bus.db");
    let out = Command::new(bin())
        .env("HOME", &home)
        .env_remove("MSGQ_TRUST")
        .env_remove("MSGQ_TRUST_SESSION")
        .arg("--db")
        .arg(&db)
        .args(["verify", "--json"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .and_then(|mut child| {
            use std::io::Write;
            child
                .stdin
                .as_mut()
                .unwrap()
                .write_all(
                    b"[1,2,3]
",
                )
                .unwrap();
            child.wait_with_output()
        })
        .unwrap();
    assert_eq!(out.status.code(), Some(0));
    let value = wrapped(String::from_utf8_lossy(&out.stdout).trim());
    assert_eq!(value["state"], "unverified");
    assert_eq!(value["reason"], "the line is not a record");
}

#[test]
fn a_conflicting_layer_is_reported_rather_than_resolved_silently() {
    let home = temp_dir();
    let db = home.join("bus.db");
    assert_eq!(run(&home, &db, &["publish", "group/a", "hi"], &[]).code, 0);
    let fingerprint = run(&home, &db, &["fingerprint"], &[])
        .stdout
        .trim()
        .to_string();

    let real = format!("alice:{fingerprint}");
    let other = format!("alice:SHA256:{}", "a".repeat(64));
    let out = run(
        &home,
        &db,
        &["read", "group/a", "--from-start", "--json"],
        &[("MSGQ_TRUST", &real), ("MSGQ_TRUST_SESSION", &other)],
    );
    // The environment wins, and the disagreement is on the record.
    let value = wrapped(out.stdout.trim());
    assert_eq!(value["verdict"]["state"], "trusted");
    assert!(out.stderr.contains("disagree"), "{}", out.stderr);
}
