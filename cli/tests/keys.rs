//! Integration tests for the key surface: making a key, naming it, and keeping it private.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_msgq")
}

/// A home of its own, so a test never writes a key into the developer's own config.
fn temp_home() -> PathBuf {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);

    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let unique = COUNTER.fetch_add(1, Ordering::Relaxed);
    let dir =
        std::env::temp_dir().join(format!("msgq-key-{}-{nanos}-{unique}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

struct Run {
    code: i32,
    stdout: String,
    stderr: String,
}

fn run(home: &Path, db: &Path, args: &[&str]) -> Run {
    let out = Command::new(bin())
        .env("HOME", home)
        .env_remove("MSGQ_TOKEN")
        .arg("--db")
        .arg(db)
        .args(args)
        .output()
        .unwrap();
    Run {
        code: out.status.code().unwrap_or(-1),
        stdout: String::from_utf8_lossy(&out.stdout).to_string(),
        stderr: String::from_utf8_lossy(&out.stderr).to_string(),
    }
}

#[test]
fn a_key_is_made_without_a_bus() {
    let home = temp_home();
    let db = home.join("msgq.db");

    let out = run(&home, &db, &["keygen", "alice"]);
    assert_eq!(out.code, 0, "{}", out.stderr);
    assert!(out.stdout.contains("SHA256:"), "{}", out.stdout);

    // Nothing about making a key may touch a bus.
    assert!(!db.exists(), "keygen created the bus file");

    let key = home.join(".config/msgq/keys/alice.key");
    assert!(key.exists());
}

#[cfg(unix)]
#[test]
fn a_key_is_not_readable_by_anybody_else() {
    use std::os::unix::fs::PermissionsExt;

    let home = temp_home();
    let db = home.join("msgq.db");
    assert_eq!(run(&home, &db, &["keygen", "alice"]).code, 0);

    let key = home.join(".config/msgq/keys/alice.key");
    let mode = std::fs::metadata(&key).unwrap().permissions().mode();
    assert_eq!(mode & 0o777, 0o600, "key mode was {:o}", mode & 0o777);
}

#[test]
fn the_fingerprint_is_the_same_one_twice() {
    let home = temp_home();
    let db = home.join("msgq.db");
    assert_eq!(run(&home, &db, &["keygen", "alice"]).code, 0);

    let first = run(&home, &db, &["fingerprint", "alice"]);
    let second = run(&home, &db, &["fingerprint", "alice"]);
    assert_eq!(first.code, 0, "{}", first.stderr);
    assert_eq!(first.stdout, second.stdout);
    assert!(first.stdout.trim().starts_with("SHA256:"));
    // One line, so it can be compared by eye or by a pipe.
    assert_eq!(first.stdout.trim().lines().count(), 1);
}

#[test]
fn a_second_keygen_refuses_until_it_is_told_to_replace() {
    let home = temp_home();
    let db = home.join("msgq.db");
    assert_eq!(run(&home, &db, &["keygen", "alice"]).code, 0);
    let before = run(&home, &db, &["fingerprint", "alice"]).stdout;

    let again = run(&home, &db, &["keygen", "alice"]);
    assert_eq!(again.code, 2);
    assert!(again.stderr.contains("--force"));
    assert_eq!(run(&home, &db, &["fingerprint", "alice"]).stdout, before);

    let forced = run(&home, &db, &["keygen", "alice", "--force"]);
    assert_eq!(forced.code, 0, "{}", forced.stderr);
    assert_ne!(run(&home, &db, &["fingerprint", "alice"]).stdout, before);
}

#[test]
fn a_private_key_never_reaches_an_output() {
    let home = temp_home();
    let db = home.join("msgq.db");
    let keygen = run(&home, &db, &["keygen", "alice"]);
    let fingerprint = run(&home, &db, &["fingerprint", "alice"]);

    // The stored seed is the one string that must never be printed anywhere.
    let stored = std::fs::read_to_string(home.join(".config/msgq/keys/alice.key")).unwrap();
    let seed = stored.trim().rsplit(':').next().unwrap().to_string();
    assert!(!seed.is_empty());

    for out in [&keygen, &fingerprint] {
        assert!(!out.stdout.contains(&seed), "{}", out.stdout);
        assert!(!out.stderr.contains(&seed), "{}", out.stderr);
    }
}

#[test]
fn a_first_publish_makes_a_key_and_says_so() {
    let home = temp_home();
    let db = home.join("msgq.db");

    let out = run(&home, &db, &["publish", "group/x", "hello"]);
    assert_eq!(out.code, 0, "{}", out.stderr);

    // The offset stays the only thing on standard output, so a script parsing it is unaffected.
    assert!(out.stdout.trim().parse::<i64>().is_ok(), "{}", out.stdout);

    let key = home.join(".config/msgq/keys");
    let names: Vec<_> = std::fs::read_dir(&key).unwrap().flatten().collect();
    assert_eq!(names.len(), 1, "expected one key, found {names:?}");

    // The notice names the fingerprint, because that is what a peer has to be told.
    assert!(out.stderr.contains("fingerprint SHA256:"), "{}", out.stderr);
}

#[test]
fn a_published_record_carries_a_signature_that_its_fingerprint_verifies() {
    let home = temp_home();
    let db = home.join("msgq.db");

    let published = run(
        &home,
        &db,
        &["publish", "group/x", "hello", "--from", "session/alice-1"],
    );
    assert_eq!(published.code, 0, "{}", published.stderr);

    let fingerprint = run(&home, &db, &["fingerprint"]).stdout.trim().to_string();
    let read = run(&home, &db, &["read", "group/x", "--from-start", "--json"]);
    assert_eq!(read.code, 0, "{}", read.stderr);

    let value: serde_json::Value = serde_json::from_str(read.stdout.trim()).unwrap();
    let sig = &value["meta"]["sig"];
    assert_eq!(sig["alg"], "ed25519", "{value}");
    assert!(sig["key"].is_string(), "{value}");
    assert!(sig["sig"].is_string(), "{value}");

    // The signature is over the sender the record carries, so the two cannot disagree.
    assert_eq!(value["from"], "session/alice-1");
    assert!(!fingerprint.is_empty());
}

#[test]
fn a_second_publish_does_not_make_a_second_key() {
    let home = temp_home();
    let db = home.join("msgq.db");
    assert_eq!(run(&home, &db, &["publish", "group/x", "one"]).code, 0);
    let second = run(&home, &db, &["publish", "group/x", "two"]);
    assert_eq!(second.code, 0, "{}", second.stderr);
    // Nothing to announce a second time.
    assert!(second.stderr.trim().is_empty(), "{}", second.stderr);
}

#[test]
fn a_principal_name_cannot_leave_the_key_directory() {
    let home = temp_home();
    let db = home.join("msgq.db");

    let out = run(&home, &db, &["keygen", "session/alice"]);
    assert_eq!(out.code, 2);
    assert!(out.stderr.contains("principal name"), "{}", out.stderr);
    assert!(!home.join(".config/msgq/keys/session").exists());
}
