//! Behavioural conformance: what the bus promises regardless of which engine stores it.
//!
//! `protocol/fixtures/behaviour/expectations.json` names the cases. Every case runs against one
//! engine, chosen by `MSGQ_BEHAVIOUR_ENGINE` (`local` by default, or `shared`). Nothing is skipped
//! in silence:
//!
//! - an engine with no runner fails the suite,
//! - the shared engine fails the suite when no database is named,
//! - a case with no runner is reported.
//!
//! "Two engines, one product" is only true if this file runs both and they agree.

use std::path::PathBuf;
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

fn shared_dsn() -> String {
    match std::env::var("MSGQ_TEST_DATABASE_URI") {
        Ok(value) if !value.trim().is_empty() => value,
        _ => panic!(
            "MSGQ_BEHAVIOUR_ENGINE=shared needs MSGQ_TEST_DATABASE_URI; \
             the shared engine is not skipped, so the suite fails instead"
        ),
    }
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

/// Where the bus lives for one case.
enum Bus {
    Local(PathBuf),
    Shared(String),
}

impl Bus {
    /// A bus the engine names, emptied of everything: the local engine gets a fresh file and the
    /// shared engine gets its queues and registry cleared, so both start from the same nothing.
    fn fresh() -> Self {
        let bus = match engine().as_str() {
            "local" => Bus::Local(temp_db()),
            "shared" => Bus::Shared(shared_dsn()),
            other => {
                panic!("engine '{other}' has no runner; add one rather than turning the suite off")
            }
        };
        bus.empty();
        bus
    }

    fn empty(&self) {
        let Bus::Shared(dsn) = self else {
            return;
        };
        let mut client = postgres::Client::connect(dsn, postgres::NoTls)
            .expect("the named test database must accept connections");
        let queues: Vec<String> = client
            .query("SELECT queue_name FROM pgmq.meta", &[])
            .expect("pgmq must be installed")
            .iter()
            .map(|row| row.get(0))
            .collect();
        for queue in queues {
            client
                .execute("SELECT pgmq.drop_queue($1)", &[&queue])
                .expect("a queue must be droppable");
        }
        client
            .batch_execute("TRUNCATE msgq.address, msgq.cursor, msgq.retention")
            .expect("the bus's own tables must be clearable");
    }

    fn location(&self) -> String {
        match self {
            Bus::Local(path) => path.display().to_string(),
            Bus::Shared(dsn) => dsn.clone(),
        }
    }

    fn run(&self, args: &[&str]) -> Run {
        let out = Command::new(bin())
            .arg("--db")
            .arg(self.location())
            .args(args)
            .output()
            .unwrap();
        Run {
            code: out.status.code().unwrap_or(-1),
            stdout: String::from_utf8_lossy(&out.stdout).to_string(),
            stderr: String::from_utf8_lossy(&out.stderr).to_string(),
        }
    }

    fn ok(&self, args: &[&str]) -> String {
        let out = self.run(args);
        assert_eq!(
            out.code, 0,
            "msgq {args:?} failed: {}\n{}",
            out.stdout, out.stderr
        );
        out.stdout
    }

    fn publish(&self, address: &str, body: &str) {
        self.ok(&["publish", address, body]);
    }

    /// Push records past a one-second window. The clock is the engine's, so the only honest way to
    /// age a record is to wait for it.
    fn age_out(&self) {
        std::thread::sleep(std::time::Duration::from_millis(2_100));
    }
}

struct Run {
    code: i32,
    stdout: String,
    stderr: String,
}

/// Run one case, or say why the active engine cannot.
fn run_case(bus: &Bus, name: &str) -> Result<(), String> {
    match name {
        "ordering" => {
            for i in 0..3 {
                bus.publish("group/a", &format!("record {i}"));
            }
            let out = bus.ok(&["read", "group/a", "--from-start"]);
            let order: Vec<&str> = out
                .lines()
                .filter_map(|line| line.split_whitespace().last())
                .collect();
            if order != ["0", "1", "2"] {
                return Err(format!("records came back out of order: {order:?}"));
            }
        }
        "cursor-monotonic" => {
            bus.publish("group/a", "one");
            bus.publish("group/a", "two");
            bus.ok(&["cursor", "set", "worker#c", "group/a", "2"]);
            bus.ok(&["cursor", "set", "worker#c", "group/a", "1"]);
            let value = bus.ok(&["cursor", "get", "worker#c", "group/a"]);
            if value.trim() != "2" {
                return Err(format!("the cursor went backwards: {value:?}"));
            }
        }
        "read-is-not-destructive" => {
            bus.publish("group/a", "one");
            bus.publish("group/a", "two");
            let first = bus.ok(&["read", "group/a", "--from-start"]);
            let second = bus.ok(&["read", "group/a", "--from-start"]);
            if first != second {
                return Err("two identical reads disagreed".to_string());
            }
            let consumers = bus.ok(&["consumers", "--json"]);
            if consumers.trim() != "[]" {
                return Err(format!("reading created a cursor: {consumers}"));
            }
        }
        "retention-window" => {
            bus.publish("group/a", "old one");
            bus.publish("group/a", "old two");
            bus.age_out();
            let out = bus.run(&["prune", "--retention", "1s", "--apply"]);
            if out.code != 0 {
                return Err(format!("prune failed: {}", out.stderr));
            }
            if !out.stdout.contains("removed 2 record(s)") {
                return Err(format!("nothing was removed: {}", out.stdout));
            }
            let after = bus.ok(&["read", "group/a", "--from-start"]);
            if !after.trim().is_empty() {
                return Err(format!("records survived the window: {after}"));
            }
        }
        "gap-reporting" => {
            // A consumer that has read nothing is the case the report exists for: everything the
            // window removed is lost to it.
            for i in 0..3 {
                bus.publish("group/a", &format!("old {i}"));
            }
            bus.age_out();
            bus.publish("group/a", "new");
            let pruned = bus.run(&["prune", "--retention", "1s", "--apply"]);
            if pruned.code != 0 {
                return Err(format!("prune failed: {}", pruned.stderr));
            }
            let positioned = bus.run(&["cursor", "set", "worker#c", "group/a", "0"]);
            if positioned.code != 0 {
                return Err(format!("cursor set failed: {}", positioned.stderr));
            }

            let consumers = bus.ok(&["consumers", "--json"]);
            if !consumers.contains("\"dropped\":3") {
                return Err(format!("the loss was not reported: {consumers}"));
            }
            let relaxed = bus.run(&["read", "group/a", "--from-start"]);
            if !relaxed
                .stderr
                .contains("3 record(s) are no longer retained")
            {
                return Err(format!("no gap warning: {}", relaxed.stderr));
            }
            let strict = bus.run(&["read", "group/a", "--from-start", "--strict"]);
            if strict.code != 1 {
                return Err(format!("--strict did not refuse: {:?}", strict.code));
            }
        }
        "address-round-trip" => {
            bus.publish("session/Api", "hi");
            let topics = bus.ok(&["topics"]);
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
        let outcome = run_case(&Bus::Local(temp_db()), &name);
        assert!(
            outcome.is_ok() || outcome.unwrap_err().starts_with("no runner"),
            "{name} has a runner that failed"
        );
    }
}

#[test]
fn behavioural_fixtures_pass_on_the_active_engine() {
    let engine = engine();
    let mut failures: Vec<String> = Vec::new();

    for name in cases() {
        let bus = Bus::fresh();
        if let Err(reason) = run_case(&bus, &name) {
            if reason.starts_with("no runner") {
                panic!(
                    "engine '{engine}': {reason}; a declared case without a runner is not a pass"
                );
            }
            failures.push(format!("{name}: {reason}"));
        }
    }

    assert!(
        failures.is_empty(),
        "behaviour differs from the fixtures on engine '{engine}': {failures:#?}"
    );
}
