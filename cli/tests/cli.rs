//! End-to-end integration tests for the msgq binary.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_msgq")
}

fn temp_db() -> PathBuf {
    // A monotonic counter along with the clock: two test threads can otherwise read the
    // same nanosecond and share one database, which silently couples unrelated tests.
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);

    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let unique = COUNTER.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("msgq-it-{}-{nanos}-{unique}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir.join("msgq.db")
}

struct Run {
    code: i32,
    stdout: String,
    stderr: String,
}

fn run(db: &Path, args: &[&str]) -> Run {
    let out = Command::new(bin())
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

fn spawn_follow(db: &Path, address: &str, consumer: &str) -> std::process::Child {
    Command::new(bin())
        .arg("--db")
        .arg(db)
        .args(["follow", address, "--consumer", consumer, "--json"])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap()
}

#[test]
fn publish_and_stateless_read() {
    let db = temp_db();
    assert_eq!(run(&db, &["publish", "group/a", "one"]).code, 0);
    assert_eq!(run(&db, &["publish", "group/a", "two"]).code, 0);

    // Default read starts at the current end: nothing new to show.
    assert_eq!(run(&db, &["read", "group/a"]).stdout.trim(), "");

    // Reading twice from the start never consumes.
    let first = run(&db, &["read", "group/a", "--from-start", "--json"]);
    let second = run(&db, &["read", "group/a", "--from-start", "--json"]);
    assert_eq!(first.stdout, second.stdout);
    assert_eq!(first.stdout.lines().count(), 2);
    for line in first.stdout.lines() {
        let value: serde_json::Value = serde_json::from_str(line).unwrap();
        assert_eq!(value["v"], 1);
    }

    // --from N returns records after N.
    let after = run(&db, &["read", "group/a", "--from", "1", "--json"]);
    assert_eq!(after.stdout.lines().count(), 1);
}

#[test]
fn topics_and_cursors() {
    let db = temp_db();
    run(&db, &["publish", "group/a", "one"]);
    run(&db, &["publish", "session/b", "two"]);

    let topics = run(&db, &["topics", "--json"]);
    let parsed: serde_json::Value = serde_json::from_str(&topics.stdout).unwrap();
    assert_eq!(parsed.as_array().unwrap().len(), 2);

    assert_eq!(
        run(&db, &["cursor", "get", "c1", "group/a"]).stdout.trim(),
        "0"
    );
    assert_eq!(
        run(&db, &["cursor", "set", "c1", "group/a", "5"])
            .stdout
            .trim(),
        "ok"
    );
    assert_eq!(
        run(&db, &["cursor", "get", "c1", "group/a"]).stdout.trim(),
        "5"
    );
    // Monotonic: a lower offset is ignored.
    assert_eq!(
        run(&db, &["cursor", "set", "c1", "group/a", "2"])
            .stdout
            .trim(),
        "ignored"
    );
    assert_eq!(
        run(&db, &["cursor", "get", "c1", "group/a"]).stdout.trim(),
        "5"
    );
    // Reset clears it.
    run(&db, &["cursor", "reset", "c1", "group/a"]);
    assert_eq!(
        run(&db, &["cursor", "get", "c1", "group/a"]).stdout.trim(),
        "0"
    );

    // Listing and removal.
    run(&db, &["cursor", "set", "c2", "group/a", "1"]);
    run(&db, &["cursor", "set", "c2", "session/b", "1"]);
    assert_eq!(run(&db, &["consumers"]).stdout.lines().count(), 2);
    assert_eq!(run(&db, &["consumers", "rm", "c2"]).stdout.trim(), "2");
    assert_eq!(run(&db, &["consumers"]).stdout.trim(), "");
}

#[test]
fn passthrough_preserves_unknown_fields() {
    let db = temp_db();
    let record =
        r#"{"v":1,"type":"note","from":"x","to":"group/p","message":"hi","custom":{"k":1}}"#;
    assert_eq!(run(&db, &["publish", "group/p", "--json", record]).code, 0);
    let out = run(&db, &["read", "group/p", "--from-start", "--json"]);
    let value: serde_json::Value = serde_json::from_str(out.stdout.trim()).unwrap();
    assert_eq!(value["custom"]["k"], 1);
    assert_eq!(value["type"], "note");
}

#[test]
fn multi_address_read_merges_by_offset() {
    let db = temp_db();
    run(&db, &["publish", "group/a", "a1"]);
    run(&db, &["publish", "group/b", "b1"]);
    run(&db, &["publish", "group/a", "a2"]);
    let out = run(
        &db,
        &["read", "group/a", "group/b", "--from-start", "--json"],
    );
    let messages: Vec<String> = out
        .stdout
        .lines()
        .map(|l| {
            let v: serde_json::Value = serde_json::from_str(l).unwrap();
            v["message"].as_str().unwrap().to_string()
        })
        .collect();
    assert_eq!(messages, vec!["a1", "b1", "a2"]);
}

#[test]
fn usage_errors_exit_two() {
    let db = temp_db();
    assert_eq!(run(&db, &["publish", "foo/bar", "x"]).code, 2);
    assert_eq!(run(&db, &["publish", "group/a"]).code, 2);
    assert_eq!(
        run(
            &db,
            &[
                "publish",
                "group/a",
                "--json",
                r#"{"v":1,"type":"note","from":"x"}"#
            ]
        )
        .code,
        2
    );
    assert_eq!(
        run(&db, &["read", "group/a", "--from", "1", "--from-start"]).code,
        2
    );
    let unknown = Command::new(bin()).arg("--nope").output().unwrap();
    assert_eq!(unknown.status.code(), Some(2));
}

#[test]
fn db_info_reports_fields() {
    let db = temp_db();
    run(&db, &["publish", "group/a", "one"]);
    let out = run(&db, &["db", "info"]);
    for field in [
        "path:",
        "existed_before:",
        "size_bytes:",
        "protocol_version:",
        "topics:",
        "messages:",
        "consumers:",
    ] {
        assert!(out.stdout.contains(field), "missing {field}");
    }
    assert!(out.stdout.contains("protocol_version: 1"));
}

#[test]
fn publish_ack_requests_acknowledgements() {
    let db = temp_db();
    assert_eq!(
        run(&db, &["publish", "cli/x", "hi", "--type", "note", "--ack"]).code,
        0
    );
    let all = run(&db, &["read", "cli/x", "--from-start", "--json"]);
    assert!(all.stdout.contains("\"ack\":true"), "{}", all.stdout);

    assert_eq!(
        run(
            &db,
            &[
                "publish",
                "cli/x",
                "hi2",
                "--type",
                "note",
                "--ack",
                "started,completed"
            ],
        )
        .code,
        0
    );
    let subset = run(&db, &["read", "cli/x", "--from-start", "--json"]);
    assert!(
        subset.stdout.contains("\"ack\":\"started,completed\""),
        "{}",
        subset.stdout
    );
}

#[test]
fn await_derives_the_reply_address_from_the_message() {
    let db = temp_db();
    let message = r#"{"v":1,"type":"note","from":"cli/bot","to":"session/w","message":"hi"}"#;
    assert_eq!(
        run(&db, &["publish", "session/w", "--json", message]).code,
        0
    );

    // The acknowledgement is addressed to the sender, not to the stream the message went to.
    let ack = r#"{"v":1,"type":"event","from":"session/w","to":"cli/bot","reply_to":1,"event":{"kind":"ack","phase":"completed","outcome":"ok"}}"#;
    assert_eq!(run(&db, &["publish", "cli/bot", "--json", ack]).code, 0);

    // No --on: the wait must find it through the referenced message's sender.
    let out = run(&db, &["await", "1", "--timeout", "5"]);
    assert_eq!(out.code, 0, "stderr: {}", out.stderr);
    assert!(out.stdout.contains("completed"), "{}", out.stdout);
}

fn spawn_ask(db: &Path, args: &[&str]) -> std::process::Child {
    Command::new(bin())
        .arg("--db")
        .arg(db)
        .arg("ask")
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap()
}

/// Wait until `topic` has a message, and return its offset. Never guess it: under
/// parallel load the ask may publish later than a fixed sleep would assume.
fn wait_for_offset(db: &Path, topic: &str) -> i64 {
    for _ in 0..100 {
        let out = run(db, &["topics", "--json"]);
        let parsed: serde_json::Value =
            serde_json::from_str(&out.stdout).unwrap_or(serde_json::Value::Null);
        if let Some(rows) = parsed.as_array() {
            for row in rows {
                if row["topic"] == topic
                    && let Some(offset) = row["max_offset"].as_i64()
                    && offset > 0
                {
                    return offset;
                }
            }
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    panic!("{topic} never received a message");
}

#[test]
fn publish_reply_to_references_an_earlier_message() {
    let db = temp_db();
    assert_eq!(run(&db, &["publish", "session/w", "question"]).code, 0);
    assert_eq!(
        run(
            &db,
            &[
                "publish",
                "cli/asker",
                "answer",
                "--type",
                "prompt",
                "--reply-to",
                "1"
            ]
        )
        .code,
        0
    );
    let out = run(&db, &["read", "cli/asker", "--from-start", "--json"]);
    assert!(out.stdout.contains("\"reply_to\":1"), "{}", out.stdout);
}

#[test]
fn ask_returns_the_reply() {
    let db = temp_db();
    let child = spawn_ask(
        &db,
        &[
            "session/w",
            "what is 2+2?",
            "--from",
            "cli/asker",
            "--timeout",
            "20",
        ],
    );

    let offset = wait_for_offset(&db, "session/w");
    let reply = format!(
        r#"{{"v":1,"type":"prompt","from":"session/w","to":"cli/asker","message":"four","reply_to":{offset}}}"#
    );
    assert_eq!(
        run(&db, &["publish", "cli/asker", "--json", &reply]).code,
        0
    );

    let out = child.wait_with_output().unwrap();
    let text = String::from_utf8_lossy(&out.stdout).to_string();
    assert_eq!(out.status.code(), Some(0));
    assert!(text.contains("four"), "{text}");
}

#[test]
fn ask_reports_finished_without_replying() {
    let db = temp_db();
    let child = spawn_ask(
        &db,
        &[
            "session/w",
            "anyone there?",
            "--from",
            "cli/asker",
            "--timeout",
            "20",
        ],
    );
    let offset = wait_for_offset(&db, "session/w");
    let ack = format!(
        r#"{{"v":1,"type":"event","from":"session/w","to":"cli/asker","reply_to":{offset},"event":{{"kind":"ack","phase":"completed","outcome":"ok"}}}}"#
    );
    assert_eq!(run(&db, &["publish", "cli/asker", "--json", &ack]).code, 0);

    let out = child.wait_with_output().unwrap();
    let text = String::from_utf8_lossy(&out.stdout).to_string();
    assert_eq!(out.status.code(), Some(0));
    assert!(text.contains("finished without replying"), "{text}");
}

#[test]
fn ask_reports_still_running_without_erroring() {
    let db = temp_db();
    let out = run(
        &db,
        &[
            "ask",
            "session/w",
            "hello?",
            "--from",
            "cli/asker",
            "--timeout",
            "1",
        ],
    );
    assert_eq!(out.code, 0, "still running is not an error: {}", out.stderr);
    assert!(out.stdout.contains("still running"), "{}", out.stdout);

    let json = run(
        &db,
        &[
            "ask",
            "session/w",
            "hello?",
            "--from",
            "cli/asker",
            "--timeout",
            "1",
            "--json",
        ],
    );
    assert!(
        json.stdout.contains("\"outcome\":\"running\""),
        "{}",
        json.stdout
    );
}

#[test]
fn acknowledgements_render_as_lifecycle_lines() {
    let db = temp_db();
    let ack = r#"{"v":1,"type":"event","from":"session/child","to":"cli/x","reply_to":42,"event":{"kind":"ack","phase":"completed","outcome":"ok"}}"#;
    assert_eq!(run(&db, &["publish", "cli/x", "--json", ack]).code, 0);

    let human = run(&db, &["read", "cli/x", "--from-start"]);
    assert!(human.stdout.contains("ack"), "{}", human.stdout);
    assert!(human.stdout.contains("#42"), "{}", human.stdout);
    assert!(human.stdout.contains("completed"), "{}", human.stdout);
    assert!(human.stdout.contains("ok"), "{}", human.stdout);

    let json = run(&db, &["read", "cli/x", "--from-start", "--json"]);
    assert!(json.stdout.contains(r#""kind":"ack""#), "{}", json.stdout);
}

#[cfg(unix)]
#[test]
fn follow_delivers_new_and_skips_history() {
    let db = temp_db();
    run(&db, &["publish", "group/a", "old"]);

    let child = spawn_follow(&db, "group/a", "c1");
    thread::sleep(Duration::from_millis(1200));
    run(&db, &["publish", "group/a", "new"]);
    thread::sleep(Duration::from_millis(1200));

    // Interrupt politely so the process saves its cursor and exits 0.
    let killed = Command::new("kill")
        .arg("-INT")
        .arg(child.id().to_string())
        .status()
        .unwrap();
    assert!(killed.success());
    let out = child.wait_with_output().unwrap();
    assert_eq!(out.status.code(), Some(0));

    let text = String::from_utf8_lossy(&out.stdout).to_string();
    assert!(text.contains("new"), "expected new in: {text}");
    assert!(!text.contains("old"), "history should not replay: {text}");

    let cursor = run(&db, &["cursor", "get", "c1", "group/a"]);
    assert!(cursor.stdout.trim().parse::<i64>().unwrap() >= 2);
}

#[cfg(unix)]
#[test]
fn duplicate_follow_is_rejected() {
    let db = temp_db();
    run(&db, &["publish", "group/a", "one"]);

    let first = spawn_follow(&db, "group/a", "c1");
    thread::sleep(Duration::from_millis(1000));

    let second = run(&db, &["follow", "group/a", "--consumer", "c1"]);
    assert_ne!(second.code, 0);
    assert!(second.stderr.contains("already following"));

    Command::new("kill")
        .arg("-INT")
        .arg(first.id().to_string())
        .status()
        .unwrap();
    let _ = first.wait_with_output();
}
