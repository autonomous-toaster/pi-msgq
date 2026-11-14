//! The documented scenario, end to end, on the shared engine.
//!
//! Two sessions on one server: a note, an ask and its acknowledgement, presence, and both rosters.
//! Everything here goes through the CLI a user would run, because the promise is about the product
//! rather than about the store's internals.

use std::process::Command;

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_msgq")
}

fn dsn() -> Option<String> {
    match std::env::var("MSGQ_TEST_DB_URI") {
        Ok(value) if !value.trim().is_empty() => Some(value),
        _ => {
            eprintln!("shared end-to-end not run: set MSGQ_TEST_DB_URI");
            None
        }
    }
}

fn unique() -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    format!("{}-{nanos}", std::process::id())
}

/// Hold the shared-engine lock for the whole scenario: this suite fills the same database.
fn exclusive(dsn: &str) -> postgres::Client {
    let mut guard =
        postgres::Client::connect(dsn, postgres::NoTls).expect("the test database must accept");
    guard
        .execute("SELECT pg_advisory_lock(1702453617)", &[])
        .expect("the shared-engine lock");
    guard
}

/// Mint a credential with scopes over the addresses this scenario uses.
fn credential(dsn: &str, name: &str) -> String {
    let mut client =
        postgres::Client::connect(dsn, postgres::NoTls).expect("the test database must accept");
    client
        .query_one(
            "SELECT msgq.register_identity($1, ARRAY['session','group','cli'])",
            &[&name],
        )
        .expect("an identity must be mintable")
        .get(0)
}

struct Run {
    code: i32,
    stdout: String,
    stderr: String,
}

/// One session's bus: its own location, credential and scope.
struct Session {
    dsn: String,
    token: String,
    scope: String,
}

impl Session {
    fn run(&self, args: &[&str]) -> Run {
        let out = Command::new(bin())
            .arg("--db")
            .arg(&self.dsn)
            .args(args)
            .env("MSGQ_TOKEN", &self.token)
            .env("MSGQ_SCOPE", &self.scope)
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
}

#[test]
fn two_sessions_exchange_a_note_an_ask_and_presence_on_one_server() {
    let Some(dsn) = dsn() else {
        return;
    };
    // One suite at a time touches this database.
    let _serialised = exclusive(&dsn);
    let unique = unique();
    let scope = format!("team{unique}");
    let alice_address = format!("session/alice-{unique}");
    let bob_address = format!("session/bob-{unique}");
    let alice = Session {
        dsn: dsn.clone(),
        token: credential(&dsn, &format!("alice-{unique}")),
        scope: scope.clone(),
    };
    let bob = Session {
        dsn: dsn.clone(),
        token: credential(&dsn, &format!("bob-{unique}")),
        scope: scope.clone(),
    };

    // The bus says what it will accept, and never prints the secret.
    let info = bob.ok(&["db", "info"]);
    assert!(info.contains("accepted as"), "{info}");
    assert!(
        !info.contains(&bob.token),
        "the credential was printed: {info}"
    );

    // A note, with the sender stamped by the server rather than claimed by the writer.
    let refused = alice.run(&[
        "publish",
        &bob_address,
        "the suite is green",
        "--from",
        "cwd/somewhere-else",
    ]);
    assert_eq!(refused.code, 1, "a claim outside the scopes is refused");
    assert!(
        refused.stderr.contains("outside the scopes"),
        "{}",
        refused.stderr
    );
    alice.ok(&[
        "publish",
        &bob_address,
        "the suite is green",
        "--from",
        &alice_address,
    ]);
    let read = bob.ok(&["read", &bob_address, "--from-start"]);
    assert!(read.contains("the suite is green"), "{read}");
    let stored = bob.ok(&["read", &bob_address, "--from-start", "--json"]);
    assert!(
        stored.contains(&alice_address),
        "the sender is the identity, not the claim: {stored}"
    );
    assert!(
        !stored.contains("cwd/somewhere-else"),
        "a refused claim stores nothing: {stored}"
    );

    // An ask, and an acknowledgement written by the other session while the ask waits. The record
    // id form is what a user copies out of the publish, and it is the form that finds the stream
    // the acknowledgement will arrive on.
    let record_id = alice
        .ok(&[
            "publish",
            &bob_address,
            "which tests?",
            "--type",
            "prompt",
            "--from",
            &alice_address,
            "--print-id",
        ])
        .trim()
        .to_string();
    assert!(
        record_id.starts_with(&bob_address),
        "the id names the address: {record_id}"
    );
    let offset: i64 = record_id
        .rsplit('#')
        .next()
        .and_then(|rest| rest.parse().ok())
        .expect("the id ends in an offset");

    let waited = std::thread::scope(|scope_thread| {
        let wait = scope_thread.spawn(|| alice.run(&["await", &record_id, "--timeout", "10"]));
        std::thread::sleep(std::time::Duration::from_millis(600));
        bob.ok(&[
            "publish",
            &alice_address,
            "--json",
            &serde_json::json!({
                "v": 1, "type": "event",
                "from": bob_address,
                "to": alice_address,
                "event": {"kind": "ack", "phase": "completed", "outcome": "answered"},
                "reply_to": offset,
            })
            .to_string(),
        ]);
        wait.join().unwrap()
    });
    assert_eq!(
        waited.code, 0,
        "the wait saw its acknowledgement: {}",
        waited.stderr
    );

    // Presence, scoped: both sessions advertise on the address of their scope.
    for (session, address, participant) in [
        (&alice, &alice_address, "alice"),
        (&bob, &bob_address, "bob"),
    ] {
        session.ok(&[
            "publish",
            &format!("group/{scope}/presence"),
            "--json",
            &serde_json::json!({
                "v": 1, "type": "event",
                "from": address,
                "to": format!("group/{scope}/presence"),
                "event": {"kind": "presence", "state": "idle", "ttl": 60,
                          "participant": participant},
            })
            .to_string(),
        ]);
    }
    let roster = bob.ok(&["sessions", "--json"]);
    assert!(roster.contains(&alice_address), "{roster}");
    assert!(roster.contains(&bob_address), "{roster}");

    // Another scope sees neither, because the address is the separation.
    let elsewhere = Session {
        dsn: dsn.clone(),
        token: credential(&dsn, &format!("out-{unique}")),
        scope: format!("other{unique}"),
    };
    let other = elsewhere.ok(&["sessions", "--json"]);
    assert!(!other.contains(&alice_address), "{other}");
    assert!(!other.contains(&bob_address), "{other}");
}
