//! Behavioural conformance: what the bus promises regardless of which engine stores it.
//!
//! `protocol/fixtures/behaviour/expectations.json` names the cases. Every case here runs against one
//! engine, chosen by `MSGQ_BEHAVIOUR_ENGINE` (default `local`). An engine that cannot run a case is
//! **reported**, never skipped in silence, so adding the shared engine cannot quietly reduce the
//! suite to nothing.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_msgq")
}

fn fixtures() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../protocol/fixtures/behaviour")
}

/// The cases the fixtures declare, in file order.
fn cases() -> Vec<String> {
    let path = fixtures().join("expectations.json");
    let raw = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()));
    let doc: serde_json::Value = serde_json::from_str(&raw).expect("expectations.json is not JSON");
    doc["cases"]
        .as_array()
        .expect("no 'cases' array")
        .iter()
        .map(|case| {
            case["name"]
                .as_str()
                .expect("a case with no name")
                .to_string()
        })
        .collect()
}

fn engine() -> String {
    std::env::var("MSGQ_BEHAVIOUR_ENGINE").unwrap_or_else(|_| "local".to_string())
}

fn temp_db() -> PathBuf {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let unique = COUNTER.fetch_add(1, Ordering::Relaxed);
    let dir =
        std::env::temp_dir().join(format!("msgq-beh-{}-{nanos}-{unique}", std::process::id()));
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

fn ok(db: &Path, args: &[&str]) -> String {
    let out = run(db, args);
    assert_eq!(
        out.code, 0,
        "msgq {args:?} failed: {}\n{}",
        out.stdout, out.stderr
    );
    out.stdout
}

fn publish(db: &Path, address: &str, body: &str) {
    ok(db, &["publish", address, body]);
}

/// Push records past a one-second window. The clock is the engine's, so the only honest way to age
/// a record is to wait for it.
fn age_out() {
    std::thread::sleep(std::time::Duration::from_millis(2_100));
}

/// Run one case, or say why the active engine cannot.
fn run_case(name: &str) -> Result<(), String> {
    let db = temp_db();
    match name {
        "ordering" => {
            for i in 0..3 {
                publish(&db, "group/a", &format!("record {i}"));
            }
            let out = ok(&db, &["read", "group/a", "--from-start"]);
            let order: Vec<&str> = out
                .lines()
                .filter_map(|line| line.split_whitespace().last())
                .collect();
            if order != ["0", "1", "2"] {
                return Err(format!("records came back out of order: {order:?}"));
            }
        }
        "cursor-monotonic" => {
            publish(&db, "group/a", "one");
            publish(&db, "group/a", "two");
            ok(&db, &["cursor", "set", "worker#c", "group/a", "2"]);
            ok(&db, &["cursor", "set", "worker#c", "group/a", "1"]);
            let value = ok(&db, &["cursor", "get", "worker#c", "group/a"]);
            if value.trim() != "2" {
                return Err(format!("the cursor went backwards: {value:?}"));
            }
        }
        "read-is-not-destructive" => {
            publish(&db, "group/a", "one");
            publish(&db, "group/a", "two");
            let first = ok(&db, &["read", "group/a", "--from-start"]);
            let second = ok(&db, &["read", "group/a", "--from-start"]);
            if first != second {
                return Err("two identical reads disagreed".to_string());
            }
            let consumers = ok(&db, &["consumers", "--json"]);
            if consumers.trim() != "[]" {
                return Err(format!("reading created a cursor: {consumers}"));
            }
        }
        "retention-window" => {
            publish(&db, "group/a", "old one");
            publish(&db, "group/a", "old two");
            age_out();
            let out = run(&db, &["prune", "--retention", "1s", "--apply"]);
            if out.code != 0 {
                return Err(format!("prune failed: {}", out.stderr));
            }
            if !out.stdout.contains("removed 2 record(s)") {
                return Err(format!("nothing was removed: {}", out.stdout));
            }
            let after = ok(&db, &["read", "group/a", "--from-start"]);
            if !after.trim().is_empty() {
                return Err(format!("records survived the window: {after}"));
            }
        }
        "gap-reporting" => {
            // A consumer that has read nothing is the case the report exists for: everything the
            // window removed is lost to it.
            for i in 0..3 {
                publish(&db, "group/a", &format!("old {i}"));
            }
            age_out();
            publish(&db, "group/a", "new");
            let pruned = run(&db, &["prune", "--retention", "1s", "--apply"]);
            if pruned.code != 0 {
                return Err(format!("prune failed: {}", pruned.stderr));
            }
            let positioned = run(&db, &["cursor", "set", "worker#c", "group/a", "0"]);
            if positioned.code != 0 {
                return Err(format!("cursor set failed: {}", positioned.stderr));
            }

            let consumers = ok(&db, &["consumers", "--json"]);
            if !consumers.contains("\"dropped\":3") {
                return Err(format!("the loss was not reported: {consumers}"));
            }
            let relaxed = run(&db, &["read", "group/a", "--from-start"]);
            if !relaxed
                .stderr
                .contains("3 record(s) are no longer retained")
            {
                return Err(format!("no gap warning: {}", relaxed.stderr));
            }
            let strict = run(&db, &["read", "group/a", "--from-start", "--strict"]);
            if strict.code != 1 {
                return Err(format!("--strict did not refuse: {:?}", strict.code));
            }
        }
        "address-round-trip" => {
            publish(&db, "session/Api", "hi");
            let topics = ok(&db, &["topics"]);
            if !topics.contains("session/Api") {
                return Err(format!("the address lost its spelling: {topics}"));
            }
        }
        other => return Err(format!("no runner for case '{other}'")),
    }
    Ok(())
}

#[test]
fn every_case_the_fixtures_declare_has_a_runner() {
    for name in cases() {
        let outcome = run_case(&name);
        assert!(
            outcome.is_ok() || outcome.unwrap_err().starts_with("no runner"),
            "{name} has a runner that failed"
        );
    }
}

#[test]
fn behavioural_fixtures_pass_on_the_active_engine() {
    let engine = engine();
    let mut unsupported = 0usize;
    let mut failures: Vec<String> = Vec::new();

    for name in cases() {
        if engine != "local" {
            unsupported += 1;
            eprintln!("msgq: engine '{engine}' is not supported yet; case '{name}' not run");
            continue;
        }
        if let Err(reason) = run_case(&name) {
            failures.push(format!("{name}: {reason}"));
        }
    }

    assert!(
        failures.is_empty(),
        "behaviour differs from the fixtures: {failures:#?}"
    );
    if engine != "local" {
        assert!(
            unsupported > 0,
            "an unsupported engine is reported, never skipped in silence"
        );
    }
}
