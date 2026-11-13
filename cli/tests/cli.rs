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

/// Run the binary with extra environment, for the settings the CLI reads from the environment.
fn run_env(db: &Path, args: &[&str], env: &[(&str, &str)]) -> Run {
    let mut command = Command::new(bin());
    command.arg("--db").arg(db).args(args);
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
fn json_with_offset_wraps_the_record() {
    let db = temp_db();
    assert_eq!(run(&db, &["publish", "group/a", "one"]).code, 0);

    let out = run(
        &db,
        &["read", "group/a", "--from-start", "--json", "--with-offset"],
    );
    assert_eq!(out.code, 0);
    assert_eq!(out.stdout.lines().count(), 1);
    let value: serde_json::Value = serde_json::from_str(out.stdout.trim()).unwrap();
    assert_eq!(value["topic"], "group/a");
    assert!(value["offset"].as_i64().unwrap() >= 1);
    assert!(value["created_at"].as_i64().unwrap() > 0);
    assert_eq!(value["record"]["message"], "one");
    assert_eq!(value["record"]["v"], 1);

    // Without the flag the raw record is byte-for-byte what it always was.
    let plain = run(&db, &["read", "group/a", "--from-start", "--json"]);
    let record: serde_json::Value = serde_json::from_str(plain.stdout.trim()).unwrap();
    assert_eq!(record["message"], "one");
    assert!(record.get("offset").is_none());
}

#[test]
fn with_offset_requires_json() {
    let db = temp_db();
    let out = run(&db, &["read", "group/a", "--with-offset"]);
    assert_eq!(out.code, 2);
}

#[test]
fn follow_with_offset_reports_the_record_position() {
    let db = temp_db();
    run(&db, &["publish", "group/a", "old"]);

    let child = Command::new(bin())
        .arg("--db")
        .arg(&db)
        .args([
            "follow",
            "group/a",
            "--consumer",
            "c1",
            "--json",
            "--with-offset",
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();

    thread::sleep(Duration::from_millis(1200));
    let published = run(&db, &["publish", "group/a", "new"]);
    let offset: i64 = published.stdout.trim().parse().unwrap();
    thread::sleep(Duration::from_millis(1200));

    Command::new("kill")
        .arg("-INT")
        .arg(child.id().to_string())
        .status()
        .unwrap();
    let out = child.wait_with_output().unwrap();
    assert_eq!(out.status.code(), Some(0));

    let text = String::from_utf8_lossy(&out.stdout).to_string();
    let line = text.lines().next().expect("one record");
    let value: serde_json::Value = serde_json::from_str(line).unwrap();
    assert_eq!(value["topic"], "group/a");
    assert_eq!(value["offset"], offset);
    assert_eq!(value["record"]["message"], "new");
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

    // A record id names the record being awaited; the wait finds who to watch from its sender.
    let out = run(&db, &["await", "session/w#1", "--timeout", "5"]);
    assert_eq!(out.code, 0, "stderr: {}", out.stderr);
    assert!(out.stdout.contains("completed"), "{}", out.stdout);
}

#[test]
fn a_connection_string_is_refused_and_creates_nothing() {
    let db = temp_db();
    let dir = db.parent().unwrap();
    let dsn = "postgres://bus@host/db";

    let out = Command::new(bin())
        .current_dir(dir)
        .args(["--db", dsn, "publish", "group/a", "hi"])
        .output()
        .unwrap();
    assert_eq!(
        out.status.code(),
        Some(2),
        "an unimplemented engine is a usage error"
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("shared engine"), "{stderr}");

    // The location is a connection string, so it must not become a file on disk.
    let created: Vec<String> = std::fs::read_dir(dir)
        .unwrap()
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .collect();
    assert!(created.is_empty(), "nothing is created: {created:?}");
}

#[test]
fn await_refuses_a_bare_offset_without_an_address() {
    let db = temp_db();
    let out = run(&db, &["await", "1", "--timeout", "1"]);
    assert_eq!(out.code, 2, "a bare offset names no record: {}", out.stderr);
    assert!(
        out.stderr
            .contains("offsets are unique only within a stream"),
        "{}",
        out.stderr
    );
}

#[test]
fn await_accepts_an_offset_with_an_address() {
    let db = temp_db();
    let message = r#"{"v":1,"type":"note","from":"cli/bot","to":"session/w","message":"hi"}"#;
    assert_eq!(
        run(&db, &["publish", "session/w", "--json", message]).code,
        0
    );
    let ack = r#"{"v":1,"type":"event","from":"session/w","to":"cli/bot","reply_to":1,"event":{"kind":"ack","phase":"completed","outcome":"ok"}}"#;
    assert_eq!(run(&db, &["publish", "cli/bot", "--json", ack]).code, 0);

    let out = run(&db, &["await", "1", "--on", "cli/bot", "--timeout", "5"]);
    assert_eq!(out.code, 0, "stderr: {}", out.stderr);
    assert!(out.stdout.contains("completed"), "{}", out.stdout);
}

#[test]
fn await_rejects_both_a_record_id_and_an_address() {
    let db = temp_db();
    let out = run(
        &db,
        &["await", "session/w#1", "--on", "cli/bot", "--timeout", "1"],
    );
    assert_eq!(out.code, 2, "{}", out.stderr);
    assert!(out.stderr.contains("--on is not needed"), "{}", out.stderr);
}

#[test]
fn await_rejects_an_id_that_names_no_record() {
    let db = temp_db();
    let out = run(&db, &["await", "session/w#7", "--timeout", "1"]);
    assert_eq!(out.code, 2, "{}", out.stderr);
    assert!(out.stderr.contains("no record at"), "{}", out.stderr);
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
fn publish_print_id_reports_the_record_id() {
    let db = temp_db();
    // The bare offset stays the default: it is what scripts already parse.
    assert_eq!(
        run(&db, &["publish", "session/a", "one"]).stdout.trim(),
        "1"
    );
    assert_eq!(
        run(&db, &["publish", "session/a", "two", "--print-id"])
            .stdout
            .trim(),
        "session/a#2"
    );
    // A bare name is canonicalized, so the id names the stream rather than what was typed.
    // Its own database: offsets are global to the database, not per stream, so the value
    // here would otherwise depend on what the earlier publishes in this test did.
    let other = temp_db();
    assert_eq!(
        run(&other, &["publish", "build.done", "three", "--print-id"])
            .stdout
            .trim(),
        "group/build.done#1"
    );
}

#[test]
fn publish_reply_to_accepts_an_id() {
    let db = temp_db();
    assert_eq!(
        run(
            &db,
            &["publish", "cli/asker", "question", "--from", "cli/asker"]
        )
        .stdout
        .trim(),
        "1"
    );

    let answer = run(
        &db,
        &[
            "publish",
            "session/w",
            "answer",
            "--from",
            "cli/asker",
            "--reply-to",
            "cli/asker#1",
        ],
    );
    assert_eq!(answer.code, 0, "{}", answer.stderr);
    let out = run(&db, &["read", "session/w", "--from-start", "--json"]);
    assert!(out.stdout.contains("\"reply_to\":1"), "{}", out.stdout);
}

#[test]
fn publish_reply_to_refuses_another_senders_id() {
    let db = temp_db();
    run(
        &db,
        &["publish", "cli/asker", "question", "--from", "cli/asker"],
    );

    // A reference points into the sender's own stream, so an id naming somebody
    // else is not a reference this record could carry.
    let out = run(
        &db,
        &[
            "publish",
            "session/w",
            "answer",
            "--from",
            "session/w",
            "--reply-to",
            "cli/asker#1",
        ],
    );
    assert_ne!(out.code, 0);
    assert!(
        out.stderr.contains("cannot answer cli/asker#1"),
        "{}",
        out.stderr
    );
    assert_eq!(
        run(&db, &["read", "session/w", "--from-start", "--json"])
            .stdout
            .trim(),
        ""
    );
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
fn ask_no_wait_returns_as_soon_as_the_question_is_published() {
    let db = temp_db();
    let began = std::time::Instant::now();
    let out = run(
        &db,
        &[
            "ask",
            "session/w",
            "what is 2+2?",
            "--from",
            "cli/asker",
            "--no-wait",
        ],
    );
    assert_eq!(out.code, 0, "stderr: {}", out.stderr);
    // The question is the only record, so the offset is the first one, and printing it is
    // what lets the caller name the record it just asked.
    assert_eq!(out.stdout.trim(), "1");
    // Nothing waited: no reply exists, and there was nothing to wait for.
    assert!(
        began.elapsed() < Duration::from_secs(5),
        "took {:?}",
        began.elapsed()
    );
    let read = run(&db, &["read", "session/w", "--from-start", "--json"]);
    assert!(read.stdout.contains("\"reply\":true"), "{}", read.stdout);
    assert!(read.stdout.contains("\"ack\":true"), "{}", read.stdout);
}

#[test]
fn ask_no_wait_publishes_the_same_record_as_a_blocking_ask() {
    let db = temp_db();
    assert_eq!(
        run(
            &db,
            &["ask", "session/w", "q", "--from", "cli/asker", "--no-wait"]
        )
        .code,
        0
    );
    // A one-second window, so the blocking ask ends without an answer.
    assert_eq!(
        run(
            &db,
            &[
                "ask",
                "session/w",
                "q",
                "--from",
                "cli/asker",
                "--timeout",
                "1",
            ]
        )
        .code,
        0
    );

    let out = run(&db, &["read", "session/w", "--from-start", "--json"]);
    let records: Vec<&str> = out
        .stdout
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect();
    assert_eq!(records.len(), 2, "{}", out.stdout);
    // Whether the sender waits is the sender's business: the wire cannot tell the two apart.
    assert_eq!(records[0], records[1]);
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

// --- the roster, derived from presence records -------------------------------------------------

/// Publish one presence record, the way any client would: a JSON event on the presence address.
fn publish_presence(db: &Path, from: &str, event: serde_json::Value) -> Run {
    let record = serde_json::json!({
        "v": 1, "type": "event", "from": from, "to": "group/presence", "event": event,
    });
    run(
        db,
        &["publish", "group/presence", "--json", &record.to_string()],
    )
}

#[test]
fn sessions_lists_a_live_presence_record() {
    let db = temp_db();
    publish_presence(
        &db,
        "session/planner",
        serde_json::json!({"kind": "presence", "state": "idle", "ttl": 30,
                           "participant": "01a1", "alias": "planner"}),
    );

    let out = run(&db, &["sessions"]);
    assert_eq!(out.code, 0, "{}", out.stderr);
    assert!(out.stdout.contains("session/planner"), "{}", out.stdout);
    assert!(out.stdout.contains("idle"), "{}", out.stdout);
    assert!(
        out.stdout.contains("session"),
        "the kind is the address scheme: {}",
        out.stdout
    );

    let json = run(&db, &["sessions", "--json"]);
    let entry: serde_json::Value =
        serde_json::from_str(json.stdout.lines().next().unwrap()).unwrap();
    assert_eq!(entry["address"], "session/planner");
    assert_eq!(entry["alias"], "planner");
    assert_eq!(entry["state"], "idle");
    assert_eq!(entry["kind"], "session");
    assert!(entry["expires_in"].as_i64().unwrap() > 0);
}

#[test]
fn card_derives_a_locator_from_a_git_worktree() {
    let db = temp_db();
    let repo = db.parent().unwrap().join("repo");
    std::fs::create_dir_all(repo.join("src")).unwrap();
    for args in [
        vec!["init", "-b", "main"],
        vec!["config", "user.email", "test@example.invalid"],
        vec!["config", "user.name", "Test"],
        vec!["remote", "add", "origin", "https://github.com/org/repo.git"],
    ] {
        let out = Command::new("git")
            .arg("-C")
            .arg(&repo)
            .args(args)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
    std::fs::write(repo.join("src").join("file.txt"), "one\n").unwrap();
    let cwd = repo.join("src");
    let cwd = cwd.to_string_lossy();
    let out = run(&db, &["card", "--cwd", &cwd, "--paths", "relative"]);
    assert_eq!(out.code, 0, "{}", out.stderr);
    let card: serde_json::Value = serde_json::from_str(out.stdout.trim()).unwrap();
    assert_eq!(card["project"], "github.com/org/repo");
    assert_eq!(card["branch"], "main");
    assert_eq!(card["worktree"], "repo");
    assert_eq!(card["path"], "src");

    let plain = run(&db, &["card", "--cwd", &cwd, "--paths", "none"]);
    let plain: serde_json::Value = serde_json::from_str(plain.stdout.trim()).unwrap();
    assert!(plain.get("path").is_none());
}

#[test]
fn sessions_reports_cards_in_json_and_wide_text() {
    let db = temp_db();
    let card = serde_json::json!({
        "project": "github.com/org/repo",
        "branch": "main",
        "worktree": "repo",
        "path": "src",
    });
    publish_presence(
        &db,
        "session/planner",
        serde_json::json!({"kind": "presence", "state": "idle", "ttl": 30,
                           "participant": "01a1", "alias": "planner", "card": card}),
    );

    let json = run(&db, &["sessions", "--json"]);
    let entry: serde_json::Value = serde_json::from_str(
        json.stdout
            .lines()
            .find(|line| line.contains("session/planner"))
            .unwrap(),
    )
    .unwrap();
    assert_eq!(entry["card"]["project"], "github.com/org/repo");
    assert_eq!(entry["card"]["branch"], "main");
    assert_eq!(entry["card"]["path"], "src");

    let plain = run(&db, &["sessions"]);
    assert!(
        !plain.stdout.contains("github.com/org/repo"),
        "{}",
        plain.stdout
    );
    let wide = run(&db, &["sessions", "--wide"]);
    let line = wide.stdout.lines().next().unwrap();
    let columns: Vec<&str> = line.split('\t').collect();
    assert_eq!(columns.len(), 6, "five columns and one locator: {line}");
    assert_eq!(
        columns[5], "github.com/org/repo  main  src",
        "the locator is one column of project, branch and path: {line}"
    );

    let at_root = serde_json::json!({
        "project": "github.com/org/repo",
        "branch": "main",
        "worktree": "repo",
        "path": ".",
    });
    publish_presence(
        &db,
        "session/reviewer",
        serde_json::json!({"kind": "presence", "state": "busy", "participant": "01a2",
                           "alias": "reviewer", "card": at_root}),
    );
    let wide = run(&db, &["sessions", "--wide"]);
    let row = wide
        .stdout
        .lines()
        .find(|line| line.starts_with("session/reviewer"))
        .unwrap();
    assert_eq!(
        row.split('\t').next_back(),
        Some("github.com/org/repo  main"),
        "a path of '.' is the repository root and adds nothing: {row}"
    );
}

#[test]
fn sessions_marks_a_participant_without_a_card_in_the_locator_column() {
    let db = temp_db();
    publish_presence(
        &db,
        "cli/ci@host",
        serde_json::json!({"kind": "presence", "state": "idle"}),
    );
    let wide = run(&db, &["sessions", "--wide"]);
    let line = wide.stdout.lines().next().unwrap();
    assert_eq!(line.split('\t').next_back(), Some("-"), "{line}");
}

#[test]
fn card_reports_the_address_beside_the_locator() {
    let db = temp_db();
    let dir = db.parent().unwrap();
    let out = run(
        &db,
        &[
            "card",
            "--cwd",
            dir.to_str().unwrap(),
            "--paths",
            "none",
            "--address",
            "session/planner",
        ],
    );
    let value: serde_json::Value = serde_json::from_str(out.stdout.trim()).unwrap();
    assert_eq!(value["address"], "session/planner");
    assert!(value["card"].is_object(), "{value}");
    assert!(
        value["card"].get("path").is_none(),
        "path mode none: {value}"
    );
    assert!(value["locator"].is_string(), "{value}");
}

#[test]
fn sessions_lists_a_client_that_is_not_a_session() {
    let db = temp_db();
    // No `participant`, no `alias`: the minimum a client has to publish to appear.
    publish_presence(
        &db,
        "cli/ci@host",
        serde_json::json!({"kind": "presence", "state": "busy", "ttl": 60}),
    );

    let json = run(&db, &["sessions", "--json"]);
    let entry: serde_json::Value =
        serde_json::from_str(json.stdout.lines().next().unwrap()).unwrap();
    assert_eq!(entry["address"], "cli/ci@host");
    assert_eq!(entry["kind"], "cli");
    assert_eq!(entry["state"], "busy");
    assert!(entry["alias"].is_null());
}

#[test]
fn sessions_excludes_an_expired_record() {
    let db = temp_db();
    publish_presence(
        &db,
        "session/gone-a-while",
        serde_json::json!({"kind": "presence", "state": "idle", "ttl": 1, "participant": "p1"}),
    );
    thread::sleep(Duration::from_millis(1200));

    let out = run(&db, &["sessions"]);
    assert_eq!(out.code, 0, "{}", out.stderr);
    assert_eq!(
        out.stdout.trim(),
        "",
        "an expired record is not a participant"
    );
}

#[test]
fn sessions_honours_a_retraction_and_a_later_return() {
    let db = temp_db();
    let from = "session/reviewer";
    publish_presence(
        &db,
        from,
        serde_json::json!({"kind": "presence", "state": "busy", "ttl": 30, "participant": "p2"}),
    );
    assert!(run(&db, &["sessions"]).stdout.contains(from));

    publish_presence(
        &db,
        from,
        serde_json::json!({"kind": "presence", "state": "gone", "ttl": 30, "participant": "p2"}),
    );
    assert_eq!(
        run(&db, &["sessions"]).stdout.trim(),
        "",
        "a retraction removes the participant at once"
    );

    publish_presence(
        &db,
        from,
        serde_json::json!({"kind": "presence", "state": "idle", "ttl": 30, "participant": "p2"}),
    );
    assert!(
        run(&db, &["sessions"]).stdout.contains(from),
        "a later record brings it back"
    );
}

#[test]
fn sessions_shows_two_participants_sharing_an_address() {
    let db = temp_db();
    let from = "session/reviewer";
    publish_presence(
        &db,
        from,
        serde_json::json!({"kind": "presence", "state": "idle", "ttl": 30, "participant": "one"}),
    );
    publish_presence(
        &db,
        from,
        serde_json::json!({"kind": "presence", "state": "busy", "ttl": 30, "participant": "two"}),
    );

    let out = run(&db, &["sessions"]);
    let rows: Vec<&str> = out
        .stdout
        .lines()
        .filter(|line| line.contains(from))
        .collect();
    assert_eq!(
        rows.len(),
        2,
        "two participants, one address, neither displaces the other: {}",
        out.stdout
    );
}

#[test]
fn sessions_keeps_the_newest_record_per_participant() {
    let db = temp_db();
    let event = |state: &str| serde_json::json!({"kind": "presence", "state": state, "ttl": 30, "participant": "p3"});
    publish_presence(&db, "session/x", event("busy"));
    publish_presence(&db, "session/x", event("idle"));

    let out = run(&db, &["sessions", "--json"]);
    let lines: Vec<&str> = out.stdout.lines().collect();
    assert_eq!(lines.len(), 1, "one entry per participant: {}", out.stdout);
    let entry: serde_json::Value = serde_json::from_str(lines[0]).unwrap();
    assert_eq!(
        entry["state"], "idle",
        "the newest record is the current state"
    );
}

#[test]
fn sessions_ignores_anything_that_is_not_presence() {
    let db = temp_db();
    run(&db, &["publish", "group/presence", "just a message"]);
    let ack = serde_json::json!({
        "v": 1, "type": "event", "from": "session/x", "to": "group/presence",
        "event": {"kind": "ack", "phase": "started"},
    });
    run(
        &db,
        &["publish", "group/presence", "--json", &ack.to_string()],
    );

    let out = run(&db, &["sessions"]);
    assert_eq!(out.code, 0, "{}", out.stderr);
    assert_eq!(
        out.stdout.trim(),
        "",
        "only presence records make a participant: {}",
        out.stdout
    );
}

#[test]
fn sessions_succeeds_when_nobody_is_online() {
    let db = temp_db();
    let out = run(&db, &["sessions"]);
    assert_eq!(out.code, 0, "{}", out.stderr);
    assert_eq!(out.stdout.trim(), "");
    assert_eq!(run(&db, &["sessions", "--json"]).stdout.trim(), "");
}

// --- retention and collection -----------------------------------------------------------------

/// Wait until a record written before this call is unambiguously older than a second.
///
/// Record times are whole seconds, so "older than 1s" needs more than a second of wall clock to be
/// true of a record written just now.
fn age_out() {
    thread::sleep(Duration::from_millis(2100));
}

#[test]
fn prune_reports_before_it_removes() {
    let db = temp_db();
    for i in 0..3 {
        run(&db, &["publish", "group/a", &format!("old {i}")]);
    }
    age_out();

    let dry = run(&db, &["prune", "--older-than", "1s"]);
    assert_eq!(dry.code, 0, "{}", dry.stderr);
    assert!(
        dry.stdout.contains("would remove 3 record(s)"),
        "{}",
        dry.stdout
    );
    assert!(dry.stdout.contains("group/a\t3 of 3"), "{}", dry.stdout);
    assert_eq!(
        run(&db, &["read", "group/a", "--from-start"])
            .stdout
            .lines()
            .count(),
        3
    );

    let applied = run(&db, &["prune", "--older-than", "1s", "--apply"]);
    assert!(
        applied.stdout.contains("removed 3 record(s)"),
        "{}",
        applied.stdout
    );
    assert_eq!(
        run(&db, &["read", "group/a", "--from-start"]).stdout.trim(),
        ""
    );
    assert_eq!(
        run(&db, &["topics"]).stdout.trim(),
        "",
        "the stream is empty"
    );
}

#[test]
fn prune_does_not_obey_a_lagging_consumer() {
    let db = temp_db();
    for i in 0..3 {
        run(&db, &["publish", "group/a", &format!("record {i}")]);
    }
    age_out();
    run(&db, &["cursor", "set", "worker#c", "group/a", "2"]);

    let out = run(&db, &["prune", "--older-than", "1s", "--apply"]);
    assert_eq!(out.code, 0, "{}", out.stderr);
    assert!(
        out.stdout.contains("removed 3 record(s)"),
        "the window wins, not the cursor: {}",
        out.stdout
    );
    assert!(
        out.stderr.contains("consumer(s) will lose records"),
        "{}",
        out.stderr
    );
    assert!(out.stderr.contains("worker#c"), "{}", out.stderr);

    assert_eq!(
        run(&db, &["read", "group/a", "--from-start"]).stdout.trim(),
        ""
    );
    let consumers = run(&db, &["consumers", "--json"]);
    assert!(
        consumers.stdout.contains("\"dropped\":3"),
        "the consumer is told what it lost: {}",
        consumers.stdout
    );
}

#[test]
fn read_reports_a_gap_and_strict_refuses_it() {
    let db = temp_db();
    // Two records age out; two arrive afterwards, so the stream is half retained.
    for i in 0..2 {
        run(&db, &["publish", "group/a", &format!("old {i}")]);
    }
    age_out();
    for i in 0..2 {
        run(&db, &["publish", "group/a", &format!("new {i}")]);
    }
    let pruned = run(&db, &["prune", "--older-than", "1s", "--apply"]);
    assert_eq!(pruned.code, 0, "{}", pruned.stderr);

    let relaxed = run(&db, &["read", "group/a", "--from-start"]);
    assert_eq!(relaxed.code, 0, "{}", relaxed.stderr);
    assert!(
        relaxed
            .stderr
            .contains("2 record(s) are no longer retained"),
        "{}",
        relaxed.stderr
    );
    assert!(relaxed.stdout.contains("new 0"), "{}", relaxed.stdout);

    let strict = run(&db, &["read", "group/a", "--from-start", "--strict"]);
    assert_eq!(
        strict.code, 1,
        "a truncated read is fatal on request: {}",
        strict.stderr
    );
    assert!(
        strict.stdout.trim().is_empty(),
        "no partial view is printed"
    );
}

#[test]
fn consumers_report_lag_and_dropped() {
    let db = temp_db();
    for i in 0..2 {
        run(&db, &["publish", "group/a", &format!("old {i}")]);
    }
    age_out();
    for i in 0..2 {
        run(&db, &["publish", "group/a", &format!("new {i}")]);
    }
    run(&db, &["prune", "--older-than", "1s", "--apply"]);
    run(&db, &["cursor", "set", "worker#c", "group/a", "1"]);

    let out = run(&db, &["consumers", "--json"]);
    assert_eq!(out.code, 0, "{}", out.stderr);
    assert!(out.stdout.contains("\"lag\":"), "{}", out.stdout);
    assert!(
        out.stdout.contains("\"dropped\":2"),
        "both retained-away records are reported: {}",
        out.stdout
    );
}

#[test]
fn db_info_names_the_backend_window_and_credential() {
    let db = temp_db();
    run(&db, &["publish", "group/a", "hello"]);
    let out = run(&db, &["db", "info"]);
    assert_eq!(out.code, 0, "{}", out.stderr);
    assert!(
        out.stdout.contains("backend:          local"),
        "{}",
        out.stdout
    );
    assert!(
        out.stdout.contains("retention_s:      3600"),
        "{}",
        out.stdout
    );
    assert!(
        out.stdout.contains("credential:       none"),
        "{}",
        out.stdout
    );
}

#[test]
fn retention_flag_and_environment_select_the_window() {
    let db = temp_db();
    for i in 0..2 {
        run(&db, &["publish", "group/a", &format!("record {i}")]);
    }
    age_out();

    let by_flag = run(&db, &["prune", "--retention", "1s"]);
    assert_eq!(by_flag.code, 0, "{}", by_flag.stderr);
    assert!(
        by_flag.stdout.contains("would remove 2 record(s)"),
        "{}",
        by_flag.stdout
    );

    let by_env = run_env(&db, &["prune"], &[("MSGQ_RETENTION", "1s")]);
    assert_eq!(by_env.code, 0, "{}", by_env.stderr);
    assert!(
        by_env.stdout.contains("would remove 2 record(s)"),
        "{}",
        by_env.stdout
    );

    let malformed = run(&db, &["prune", "--retention", "soon"]);
    assert_eq!(malformed.code, 2, "{}", malformed.stderr);
}

#[test]
fn prune_keeps_the_newest_presence_record_of_each_participant() {
    let db = temp_db();
    publish_presence(
        &db,
        "session/x",
        serde_json::json!({"kind": "presence", "state": "idle", "ttl": 600, "participant": "p1"}),
    );
    publish_presence(
        &db,
        "session/x",
        serde_json::json!({"kind": "presence", "state": "busy", "ttl": 600, "participant": "p1"}),
    );
    publish_presence(
        &db,
        "session/y",
        serde_json::json!({"kind": "presence", "state": "idle", "ttl": 600, "participant": "p2"}),
    );
    age_out();

    let out = run(&db, &["prune", "--older-than", "1s", "--apply"]);
    assert_eq!(out.code, 0, "{}", out.stderr);
    assert!(
        out.stdout.contains("removed 1 record(s)"),
        "one of the two records for p1 goes: {}",
        out.stdout
    );

    let roster = run(&db, &["sessions", "--json"]).stdout;
    assert!(
        roster.contains("session/x"),
        "the newest record of p1 survives: {roster}"
    );
    assert!(
        roster.contains("\"state\":\"busy\""),
        "and it is the newest one: {roster}"
    );
    assert!(roster.contains("session/y"), "{roster}");
}

/// A bus's footprint: the file plus the write-ahead log that stands in for it while it is open.
fn on_disk(db: &Path) -> u64 {
    let main = std::fs::metadata(db).map(|m| m.len()).unwrap_or(0);
    let mut wal = db.as_os_str().to_os_string();
    wal.push("-wal");
    main + std::fs::metadata(PathBuf::from(wal))
        .map(|m| m.len())
        .unwrap_or(0)
}

#[test]
fn prune_returns_the_space_it_freed() {
    let db = temp_db();
    let payload = "x".repeat(8_000);
    for i in 0..20 {
        run(&db, &["publish", "group/big", &format!("{i} {payload}")]);
    }
    age_out();
    let before = on_disk(&db);
    assert!(before > 100_000, "the records are on disk: {before}");

    let out = run(&db, &["prune", "--older-than", "1s", "--apply"]);
    assert_eq!(out.code, 0, "{}", out.stderr);
    let after = on_disk(&db);
    assert!(
        after < before / 2,
        "the space comes back once the records and the log are compacted: {after} < {before}"
    );
}

#[test]
fn collection_removes_a_dead_sessions_cursors_and_nothing_else() {
    let db = temp_db();
    let dead = "00000000-0000-7000-8000-00000000000a";
    let live = "00000000-0000-7000-8000-00000000000b";
    for i in 0..2 {
        run(&db, &["publish", "group/a", &format!("record {i}")]);
    }
    run(&db, &["cursor", "set", dead, "group/a", "1"]);
    run(
        &db,
        &["cursor", "set", &format!("alias#{dead}"), "group/a", "1"],
    );
    run(&db, &["cursor", "set", live, "group/a", "1"]);
    run(&db, &["cursor", "set", "ci-bot", "group/a", "1"]);
    publish_presence(
        &db,
        &format!("session/{live}"),
        serde_json::json!({"kind": "presence", "state": "idle", "ttl": 600, "participant": live}),
    );

    let dry = run(&db, &["consumers", "gc"]);
    assert_eq!(
        dry.stdout.trim(),
        "2",
        "the dead session's two cursors: {}",
        dry.stdout
    );
    assert!(dry.stderr.contains("--apply"), "{}", dry.stderr);
    assert_eq!(
        run(&db, &["consumers"]).stdout.lines().count(),
        4,
        "nothing was removed yet"
    );

    let applied = run(&db, &["consumers", "gc", "--apply"]);
    assert_eq!(applied.stdout.trim(), "2", "{}", applied.stdout);
    let left = run(&db, &["consumers"]).stdout;
    assert!(
        !left.contains(dead),
        "the dead session's cursors are gone: {left}"
    );
    assert!(
        left.contains(live),
        "the live session keeps its cursor: {left}"
    );
    assert!(
        left.contains("ci-bot"),
        "a script's cursor is not a session's to collect: {left}"
    );
}

#[test]
fn db_info_reports_lag_and_record_age() {
    let db = temp_db();
    for i in 0..3 {
        run(&db, &["publish", "group/a", &format!("record {i}")]);
    }
    run(&db, &["cursor", "set", "worker", "group/a", "1"]);

    let out = run(&db, &["db", "info"]);
    assert_eq!(out.code, 0, "{}", out.stderr);
    assert!(
        out.stdout
            .contains("consumers:        name\ttopic\tcursor\tlag"),
        "{}",
        out.stdout
    );
    let worker = out
        .stdout
        .lines()
        .find(|line| line.trim_start().starts_with("worker\tgroup/a"))
        .unwrap_or_else(|| panic!("no consumer row: {}", out.stdout));
    assert!(
        worker.trim_end().ends_with('2'),
        "a cursor at 1 of 3 lags by two: {worker}"
    );
    assert_eq!(
        out.stdout
            .lines()
            .filter(|line| line.starts_with("  group/a\t"))
            .count(),
        1,
        "streams carry their age: {}",
        out.stdout
    );
    assert!(out.stdout.contains("presence_age_s:"), "{}", out.stdout);
}

#[test]
fn maintenance_bounds_a_bus_with_live_and_dead_participants() {
    let db = temp_db();
    let live = "00000000-0000-7000-8000-0000000000c1";
    let dead = "00000000-0000-7000-8000-0000000000c2";
    let payload = "y".repeat(4_000);

    for i in 0..10 {
        run(&db, &["publish", "group/a", &format!("{i} {payload}")]);
    }
    run(&db, &["cursor", "set", live, "group/a", "10"]);
    run(&db, &["cursor", "set", dead, "group/a", "10"]);
    publish_presence(
        &db,
        &format!("session/{live}"),
        serde_json::json!({"kind": "presence", "state": "idle", "ttl": 600, "participant": live}),
    );
    age_out();

    let before = run(&db, &["db", "info"]);
    assert!(
        before.stdout.contains("group/a\t10\t10"),
        "{}",
        before.stdout
    );
    let before_bytes = on_disk(&db);

    // Both cursors are at the head, so the records are nobody's owed work and the dead session's
    // cursor is the only thing left of it.
    let pruned = run(&db, &["prune", "--older-than", "1s", "--apply"]);
    assert_eq!(pruned.code, 0, "{}", pruned.stderr);
    let collected = run(&db, &["consumers", "gc", "--apply"]);
    assert_eq!(collected.stdout.trim(), "1", "{}", collected.stdout);

    let after = run(&db, &["db", "info"]);
    assert!(
        after.stdout.contains("messages:         1"),
        "the presence window stays: {}",
        after.stdout
    );
    assert!(
        on_disk(&db) < before_bytes / 2,
        "and the bus is smaller than it was"
    );
    let consumers = run(&db, &["consumers"]).stdout;
    assert!(
        consumers.contains(live),
        "the live session keeps its cursor: {consumers}"
    );
    assert!(
        !consumers.contains(dead),
        "the dead one does not: {consumers}"
    );
    let roster = run(&db, &["sessions", "--json"]).stdout;
    assert!(roster.contains(&format!("session/{live}")), "{roster}");
}
