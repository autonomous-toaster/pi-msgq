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
fn prune_keeps_what_a_consumer_has_not_passed() {
    let db = temp_db();
    for i in 0..3 {
        run(&db, &["publish", "group/a", &format!("record {i}")]);
    }
    age_out();
    // A cursor names the last record its consumer has taken responsibility for. Offsets are 1-based,
    // so a cursor at 2 covers the first two records and the third is still owed.
    run(&db, &["cursor", "set", "worker#c", "group/a", "2"]);

    let out = run(&db, &["prune", "--older-than", "1s", "--apply"]);
    assert_eq!(out.code, 0, "{}", out.stderr);
    assert!(out.stdout.contains("removed 2 record(s)"), "{}", out.stdout);
    assert!(
        out.stderr
            .contains("kept because a consumer has not passed"),
        "{}",
        out.stderr
    );

    let remaining = run(&db, &["read", "group/a", "--from-start"]).stdout;
    assert!(
        remaining.contains("record 2"),
        "the record past the cursor is still there: {remaining}"
    );
    assert!(!remaining.contains("record 0"), "{remaining}");
    assert!(!remaining.contains("record 1"), "{remaining}");
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
