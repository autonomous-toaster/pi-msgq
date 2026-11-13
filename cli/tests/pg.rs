//! Tests for the shared engine (pgmq on PostgreSQL).
//!
//! These need a live server with the pgmq schema installed. They are gated on
//! `MSGQ_TEST_DATABASE_URI` and say so when the variable is absent, rather than passing quietly:
//! a shared-engine test that never ran is not evidence.

use msgq::config::Target;
use msgq::pg::queue_name;
use msgq::store::{Bus, Store};
use postgres::{Client, NoTls};
use serde_json::json;

/// The test database, when one was named.
fn dsn() -> Option<String> {
    match std::env::var("MSGQ_TEST_DATABASE_URI") {
        Ok(value) if !value.trim().is_empty() => Some(value),
        _ => {
            eprintln!(
                "shared engine tests not run: set MSGQ_TEST_DATABASE_URI to a database with pgmq"
            );
            None
        }
    }
}

/// Serialise these tests: they read and compare global engine state, so overlapping runs measure
/// each other. The lock is session-scoped and released when the returned client drops.
fn exclusive(dsn: &str) -> Client {
    let mut guard = client(dsn);
    guard
        .execute("SELECT pg_advisory_lock(1702453617)", &[])
        .unwrap();
    guard
}

/// A credential with scopes over every address these tests use, minted fresh for the run.
fn fixture_token(dsn: &str) -> String {
    let mut client = client(dsn);
    client
        .query_one(
            "SELECT msgq.register_identity('cli/fixture', ARRAY['group','session','cwd'])",
            &[],
        )
        .unwrap()
        .get(0)
}

/// The bus, presenting the fixture credential.
fn open(dsn: &str) -> Store {
    Store::open_with(&Target::Shared(dsn.to_string()), Some(fixture_token(dsn)))
        .expect("the shared engine must open")
        .0
}

fn client(dsn: &str) -> Client {
    Client::connect(dsn, NoTls).expect("the named test database must accept connections")
}

/// Whether an address has a queue, and whether the engine's registry knows it. Counted per address
/// rather than globally: other suites share this database, so a global count measures them too.
fn footprint(client: &mut Client, address: &str) -> (i64, i64) {
    let name = msgq::pg::queue_name(address);
    let queues: i64 = client
        .query_one(
            "SELECT COUNT(*) FROM pg_tables WHERE schemaname = 'pgmq' AND tablename = $1",
            &[&format!("q_{name}")],
        )
        .unwrap()
        .get(0);
    let registry: i64 = client
        .query_one(
            "SELECT COUNT(*) FROM pgmq.meta WHERE queue_name = $1",
            &[&name],
        )
        .unwrap()
        .get(0);
    (queues, registry)
}

#[test]
fn a_fresh_connection_bootstraps_only_the_bus_tables() {
    let Some(dsn) = dsn() else {
        return;
    };
    let _serialised = exclusive(&dsn);
    let mut raw = client(&dsn);
    let untouched = format!("group/untouched-{}", unique());
    let before = footprint(&mut raw, &untouched);

    let (store, created) =
        Store::open_with(&Target::Shared(dsn.clone()), None).expect("the shared engine must open");
    assert!(!created, "opening a shared bus creates no file");

    assert_eq!(store.engine(), "shared");

    // The bus's own tables are the whole of what it adds.
    for table in [
        "msgq.address",
        "msgq.cursor",
        "msgq.meta",
        "msgq.retention",
        "msgq.identity",
    ] {
        let present: bool = raw
            .query_one("SELECT to_regclass($1) IS NOT NULL", &[&table])
            .unwrap()
            .get(0);
        assert!(present, "{table} must exist after opening");
    }

    // What it records about itself.
    assert_eq!(
        store.meta("protocol_version").unwrap().as_deref(),
        Some(msgq::version())
    );
    assert!(store.meta("created_by").unwrap().is_some());
    assert!(
        store.meta("pgmq_version").unwrap().is_some(),
        "the engine version is recorded, so a later migration can tell what it talks to"
    );

    // And nothing else: no queue was created and the engine's registry is untouched.
    let after = footprint(&mut raw, &untouched);
    assert_eq!(
        before, after,
        "a fresh connection creates no queue and adds no registry row"
    );
    assert_eq!(after, (0, 0), "nothing was there to begin with");
}

#[test]
fn opening_twice_records_one_set_of_facts() {
    let Some(dsn) = dsn() else {
        return;
    };
    let _serialised = exclusive(&dsn);
    let first = open(&dsn);
    drop(first);
    let second = open(&dsn);

    let mut raw = client(&dsn);
    let versions: i64 = raw
        .query_one(
            "SELECT COUNT(*) FROM msgq.meta WHERE key = 'protocol_version'",
            &[],
        )
        .unwrap()
        .get(0);
    assert_eq!(
        versions, 1,
        "the facts are recorded once, not per connection"
    );
    assert!(second.meta("protocol_version").unwrap().is_some());
}

/// A suffix that keeps each run's addresses apart without colliding across runs.
fn unique() -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    format!("{}-{nanos}", std::process::id())
}

/// The queue a registered address maps to.
fn queue_for(raw: &mut Client, address: &str) -> Option<String> {
    raw.query_opt(
        "SELECT queue_name FROM msgq.address WHERE address = $1",
        &[&address],
    )
    .unwrap()
    .map(|row| row.get(0))
}

/// The headers of the record the engine stored.
fn headers_of(raw: &mut Client, queue: &str, offset: i64) -> serde_json::Value {
    let row = raw
        .query_one(
            &format!("SELECT headers::text FROM pgmq.\"q_{queue}\" WHERE msg_id = $1"),
            &[&offset],
        )
        .unwrap();
    let text: Option<String> = row.get(0);
    serde_json::from_str(&text.expect("a stored record carries headers")).unwrap()
}

/// The canonical spelling the bus stores: no surrounding space or slash, no doubled slash, and
/// case preserved. A test that looks an address up has to use it.
fn canonical(address: &str) -> String {
    let trimmed = address
        .trim_matches(|c: char| c == ' ' || c == '/')
        .to_string();
    let mut out = String::new();
    let mut after_slash = false;
    for c in trimmed.chars() {
        if c == '/' {
            if !after_slash {
                out.push(c);
            }
            after_slash = true;
        } else {
            out.push(c);
            after_slash = false;
        }
    }
    out
}

/// Publish one record and return the queue it landed in and the record's headers.
fn publish_and_stamp(dsn: &str, address: &str) -> (String, serde_json::Value) {
    let store = open(dsn);
    let offset = store
        .publish(
            address,
            &json!({"v": 1, "type": "note", "from": "cli/t@h", "to": address, "message": "hi"}),
        )
        .unwrap();
    let mut raw = client(dsn);
    let queue = queue_for(&mut raw, &canonical(address)).expect("publishing registers the address");
    let headers = headers_of(&mut raw, &queue, offset);
    (queue, headers)
}

#[test]
fn the_address_survives_a_long_path_and_a_case_pair() {
    let Some(dsn) = dsn() else {
        return;
    };
    let _serialised = exclusive(&dsn);
    let unique = unique();

    // A long cwd address is the case a name cannot be passed through: pgmq lowercases and caps the
    // table name, and a cwd address is neither lower nor short.
    let long = format!("cwd/Users/Somebody/src/{unique}/very-long-segment-name-here/deeper/still");
    let (queue, headers) = publish_and_stamp(&dsn, &long);
    assert!(queue.len() <= 47, "{queue} exceeds the engine's limit");
    assert_eq!(
        headers["msgq"]["address"],
        json!(long),
        "the record carries the address, with its spelling intact"
    );

    // Spelling is part of an address, exactly as it is in the CLI: the shared engine does not
    // quietly lowercase a name the local engine keeps.
    let upper = format!("session/Api{unique}");
    let lower = format!("session/api{unique}");
    let (upper_queue, upper_headers) = publish_and_stamp(&dsn, &upper);
    let (lower_queue, lower_headers) = publish_and_stamp(&dsn, &lower);
    assert_ne!(
        upper_queue, lower_queue,
        "a case-differing pair stays distinct"
    );
    assert_eq!(upper_headers["msgq"]["address"], json!(upper));
    assert_eq!(lower_headers["msgq"]["address"], json!(lower));

    let mut raw = client(&dsn);
    let rows: i64 = raw
        .query_one(
            "SELECT COUNT(*) FROM msgq.address WHERE address = ANY($1)",
            &[&vec![upper, lower]],
        )
        .unwrap()
        .get(0);
    assert_eq!(rows, 2, "each spelling is its own address");
}

#[test]
fn addresses_whose_slugs_collide_still_get_their_own_queue() {
    let Some(dsn) = dsn() else {
        return;
    };
    let _serialised = exclusive(&dsn);
    let unique = unique();
    // The slug maps every non-alphanumeric character to a dot, so these two addresses read the same
    // and only the digest can separate them.
    let dotted = format!("group/a.b{unique}");
    let dashed = format!("group/a-b{unique}");
    assert_eq!(
        queue_name(&dotted).len(),
        queue_name(&dashed).len(),
        "same shape, so the readability of the name is identical"
    );
    let (dotted_queue, dotted_headers) = publish_and_stamp(&dsn, &dotted);
    let (dashed_queue, dashed_headers) = publish_and_stamp(&dsn, &dashed);
    assert_ne!(
        dotted_queue, dashed_queue,
        "the digest, not the readable part, is the identity"
    );
    assert_eq!(dotted_headers["msgq"]["address"], json!(dotted));
    assert_eq!(dashed_headers["msgq"]["address"], json!(dashed));
}

/// A queue's identity, so a test can prove no DDL ran.
fn queue_oid(raw: &mut Client, queue: &str) -> Option<u32> {
    raw.query_opt(
        &format!("SELECT oid FROM pg_class WHERE oid = to_regclass(\'pgmq.\"q_{queue}\"\')"),
        &[],
    )
    .unwrap()
    .map(|row| row.get(0))
}

/// A checksum over the whole queue, including the columns a lease would touch.
fn queue_checksum(raw: &mut Client, queue: &str) -> String {
    raw.query_one(
        &format!(
            "SELECT COALESCE(md5(string_agg(msg_id::text || \':\' || vt::text || \':\' ||
                    enqueued_at::text || \':\' || read_ct::text, \',\' ORDER BY msg_id)), \'empty\')
             FROM pgmq.\"q_{queue}\""
        ),
        &[],
    )
    .unwrap()
    .get(0)
}

#[test]
fn reading_an_unpublished_address_creates_nothing() {
    let Some(dsn) = dsn() else {
        return;
    };
    let _serialised = exclusive(&dsn);
    let store = open(&dsn);
    let address = format!("group/never-published-{}", unique());
    let mut raw = client(&dsn);
    let queues_before: i64 = raw
        .query_one(
            "SELECT COUNT(*) FROM pg_tables WHERE schemaname = \'pgmq\'",
            &[],
        )
        .unwrap()
        .get(0);

    assert!(store.read_since(&address, 0, 10).unwrap().is_empty());
    assert_eq!(store.head(&address).unwrap(), 0);
    assert_eq!(store.oldest_offset(&address).unwrap(), None);

    assert_eq!(
        queue_for(&mut raw, &address),
        None,
        "a read registers no address"
    );
    let queues_after: i64 = raw
        .query_one(
            "SELECT COUNT(*) FROM pg_tables WHERE schemaname = \'pgmq\'",
            &[],
        )
        .unwrap()
        .get(0);
    assert_eq!(queues_before, queues_after, "a read creates no queue");
}

#[test]
fn two_first_publishers_both_succeed() {
    let Some(dsn) = dsn() else {
        return;
    };
    let _serialised = exclusive(&dsn);
    let address = format!("group/race-{}", unique());
    let token = fixture_token(&dsn);

    let mut workers = Vec::new();
    for index in 0..2 {
        let dsn = dsn.clone();
        let address = address.clone();
        let token = token.clone();
        workers.push(std::thread::spawn(move || {
            let store = Store::open_with(&Target::Shared(dsn), Some(token))
                .expect("the shared engine must open")
                .0;
            store
                .publish(
                    &address,
                    &json!({"v": 1, "type": "note", "from": format!("cli/w{index}@h"),
                            "to": address, "message": "hi"}),
                )
                .unwrap();
        }));
    }
    for worker in workers {
        worker
            .join()
            .expect("a first publish must not fail because another was first");
    }

    let mut raw = client(&dsn);
    let queue = queue_for(&mut raw, &address).expect("both publishes name one address");
    let count: i64 = raw
        .query_one(&format!("SELECT COUNT(*) FROM pgmq.\"q_{queue}\""), &[])
        .unwrap()
        .get(0);
    assert_eq!(count, 2, "both records are in the one queue");
    let registrations: i64 = raw
        .query_one(
            "SELECT COUNT(*) FROM msgq.address WHERE address = $1",
            &[&address],
        )
        .unwrap()
        .get(0);
    assert_eq!(registrations, 1, "one address, one registration");
}

#[test]
fn a_second_publish_runs_no_ddl() {
    let Some(dsn) = dsn() else {
        return;
    };
    let _serialised = exclusive(&dsn);
    let address = format!("group/steady-{}", unique());
    let (queue, _) = publish_and_stamp(&dsn, &address);

    let mut raw = client(&dsn);
    let oid_before = queue_oid(&mut raw, &queue).expect("the queue exists");
    let store = open(&dsn);
    for _ in 0..3 {
        store
            .publish(
                &address,
                &json!({"v": 1, "type": "note", "from": "cli/a@h", "to": address, "message": "again"}),
            )
            .unwrap();
    }
    assert_eq!(
        queue_oid(&mut raw, &queue),
        Some(oid_before),
        "a known address reuses its queue rather than recreating it"
    );
}

#[test]
fn reading_twice_changes_nothing() {
    let Some(dsn) = dsn() else {
        return;
    };
    let _serialised = exclusive(&dsn);
    let address = format!("group/readonly-{}", unique());
    let (queue, _) = publish_and_stamp(&dsn, &address);
    let store = open(&dsn);
    store
        .publish(
            &address,
            &json!({"v": 1, "type": "note", "from": "cli/a@h", "to": address, "message": "two"}),
        )
        .unwrap();

    let mut raw = client(&dsn);
    let before = queue_checksum(&mut raw, &queue);
    let first = store.read_since(&address, 0, 10).unwrap();
    let second = store.read_since(&address, 0, 10).unwrap();
    let after = queue_checksum(&mut raw, &queue);

    assert_eq!(first.len(), 2);
    assert_eq!(
        first.iter().map(|r| r.offset).collect::<Vec<_>>(),
        second.iter().map(|r| r.offset).collect::<Vec<_>>(),
        "the same read returns the same records"
    );
    assert_eq!(
        before, after,
        "a read leaves no trace: no lease, no counter"
    );
    assert_eq!(store.head(&address).unwrap(), 2);
}

#[test]
fn a_cursor_never_moves_backwards_and_reading_never_moves_it() {
    let Some(dsn) = dsn() else {
        return;
    };
    let _serialised = exclusive(&dsn);
    let address = format!("group/cursor-{}", unique());
    let (_, _) = publish_and_stamp(&dsn, &address);
    let store = open(&dsn);
    let consumer = format!("cli/cursor-{}", unique());

    assert!(store.cursor_set(&consumer, &address, 5).unwrap());
    assert!(
        !store.cursor_set(&consumer, &address, 3).unwrap(),
        "a lower value is refused"
    );
    assert_eq!(store.cursor_get(&consumer, &address).unwrap(), Some(5));

    let _ = store.read_since(&address, 0, 10).unwrap();
    assert_eq!(
        store.cursor_get(&consumer, &address).unwrap(),
        Some(5),
        "a read is not a delivery, so it advances nothing"
    );
    assert_eq!(store.cursor_reset(&consumer, &address).unwrap(), 1);
    assert_eq!(store.cursor_get(&consumer, &address).unwrap(), None);
}

#[test]
fn a_publish_wakes_a_watcher_under_a_second() {
    let Some(dsn) = dsn() else {
        return;
    };
    let _serialised = exclusive(&dsn);
    let address = format!("group/wake-{}", unique());
    let store = open(&dsn);
    store.ensure_address(&address).unwrap();

    let watching = store.wake(&[&address]);
    let publisher = {
        let dsn = dsn.clone();
        let address = address.clone();
        std::thread::spawn(move || {
            // Give the watcher time to subscribe before the record exists.
            std::thread::sleep(std::time::Duration::from_millis(200));
            let store = open(&dsn);
            store
                .publish(
                    &address,
                    &json!({"v": 1, "type": "note", "from": "cli/a@h", "to": address,
                            "message": "wake up"}),
                )
                .unwrap();
        })
    };

    let started = std::time::Instant::now();
    watching
        .wait(std::time::Duration::from_secs(5))
        .expect("a watcher wakes or times out, never fails");
    let elapsed = started.elapsed();
    publisher.join().unwrap();
    assert!(
        elapsed < std::time::Duration::from_secs(1),
        "a notification wakes the watcher instead of waiting out the slice: {elapsed:?}"
    );
}

#[test]
fn a_wake_with_no_publication_returns_after_one_slice() {
    let Some(dsn) = dsn() else {
        return;
    };
    let _serialised = exclusive(&dsn);
    let address = format!("group/quiet-{}", unique());
    let store = open(&dsn);
    store.ensure_address(&address).unwrap();

    // This is the polling path in disguise: nothing will ever notify, so the slice is the bound.
    let watching = store.wake(&[&address]);
    let started = std::time::Instant::now();
    watching
        .wait(std::time::Duration::from_millis(120))
        .expect("a quiet bus is not an error");
    assert!(started.elapsed() >= std::time::Duration::from_millis(100));
}

#[test]
fn a_prune_reports_its_total_and_keeps_the_presence_pin() {
    let Some(dsn) = dsn() else {
        return;
    };
    let _serialised = exclusive(&dsn);
    let address = format!("group/prune-{}", unique());
    let store = open(&dsn);

    // Presence records for three participants, interleaved with ordinary ones.
    let mut presence = Vec::new();
    for index in 0..60 {
        let participant = format!("cli/p{}@h", index % 3);
        let record = json!({"v": 1, "type": "event",
                            "event": {"kind": "presence", "state": "online",
                                      "participant": participant},
                            "from": "cli/a@h", "to": address});
        let offset = store.publish(&address, &record).unwrap();
        if index % 3 == 0 {
            presence.push(offset);
        }
    }
    let head = store.head(&address).unwrap();
    let keep: Vec<i64> = store.newest_presence_offsets(&address).unwrap();
    assert_eq!(keep.len(), 3, "one pin per participant");

    // A batch smaller than the prefix, so the removal cannot happen in one statement.
    let removal = store.prune_before(&address, head, &keep, 25).unwrap();
    assert!(
        removal.batches > 1,
        "a large prefix is removed in bounded batches: {removal:?}"
    );
    assert_eq!(
        removal.removed + keep.len() as usize,
        60,
        "everything but the pins is removed, and the total is reported"
    );

    let left = store.read_since(&address, 0, 100).unwrap();
    let surviving: Vec<i64> = left.iter().map(|r| r.offset).collect();
    for offset in &keep {
        assert!(
            surviving.contains(offset),
            "a pin survives the prune: {offset}"
        );
    }
    assert_eq!(surviving.len(), keep.len());

    // And the watermark records what happened, so a lagging consumer can be told.
    store.retention_record(&address, head, 0).unwrap();
    let dropped = store
        .retention_record(&address, head, removal.removed as i64)
        .and(store.retention_watermark(&address))
        .unwrap()
        .expect("a prune leaves a watermark");
    assert_eq!(dropped, (head, removal.removed as i64));
}

#[test]
fn an_address_whose_records_aged_out_is_still_listed() {
    let Some(dsn) = dsn() else {
        return;
    };
    let _serialised = exclusive(&dsn);
    let address = format!("group/aged-{}", unique());
    let store = open(&dsn);
    let offset = store
        .publish(
            &address,
            &json!({"v": 1, "type": "note", "from": "cli/a@h", "to": address, "message": "x"}),
        )
        .unwrap();

    // A consumer that has read nothing yet is the reason the loss must be visible.
    let consumer = format!("cli/aged-{}", unique());
    store.cursor_set(&consumer, &address, 0).unwrap();
    let removal = store.prune_before(&address, offset, &[], 100).unwrap();
    assert_eq!(removal.removed, 1);
    store.retention_record(&address, offset, 1).unwrap();

    let listed = store.list_addresses().unwrap();
    let found = listed
        .iter()
        .find(|(name, _, _)| name == &address)
        .expect("an address with no records is still an address");
    assert_eq!(found.1, 0, "the count is the records left, so zero");
    assert_eq!(found.2, offset, "the newest offset is remembered");

    let consumers = store.list_consumers(Some(&address)).unwrap();
    let (name, _, last) = consumers
        .iter()
        .find(|(name, _, _)| name == &consumer)
        .expect("the lagging consumer is listed");
    assert_eq!(name, &consumer);
    assert_eq!(*last, 0, "the consumer never read anything");
    assert_eq!(
        store.retention_dropped(&address, *last).unwrap(),
        1,
        "the consumer is told what it lost"
    );
}

#[test]
fn a_summary_reports_records_and_the_age_of_the_oldest() {
    let Some(dsn) = dsn() else {
        return;
    };
    let _serialised = exclusive(&dsn);
    let address = format!("group/summary-{}", unique());
    let (_, _) = publish_and_stamp(&dsn, &address);
    let store = open(&dsn);
    store
        .publish(
            &address,
            &json!({"v": 1, "type": "note", "from": "cli/a@h", "to": address, "message": "two"}),
        )
        .unwrap();

    let summaries = store.stream_summaries().unwrap();
    let summary = summaries
        .iter()
        .find(|summary| summary.topic == address)
        .expect("the address is summarised");
    assert_eq!(summary.records, 2);
    assert_eq!(summary.newest_offset, 2);
    assert!(
        summary.oldest_at > 0,
        "the summary carries a real timestamp, not zero"
    );
}

#[test]
fn rust_and_sql_agree_on_a_queue_name() {
    let Some(dsn) = dsn() else {
        return;
    };
    let _serialised = exclusive(&dsn);
    let mut raw = client(&dsn);
    for address in [
        "session/alice",
        "group/a.b",
        "cwd/users/somebody/src/very-long-segment-name-here/deeper/still",
        "g",
    ] {
        let from_sql: String = raw
            .query_one("SELECT msgq.queue_name($1)", &[&address])
            .unwrap()
            .get(0);
        assert_eq!(
            from_sql,
            msgq::pg::queue_name(address),
            "the derived name has one definition, and it is checked here: {address}"
        );
    }
}

#[test]
fn a_credential_is_never_stored() {
    let Some(dsn) = dsn() else {
        return;
    };
    let _serialised = exclusive(&dsn);
    let address = format!("group/secret-{}", unique());
    let token = fixture_token(&dsn);
    let mut raw = client(&dsn);
    let digest: String = raw
        .query_one(
            "SELECT encode(msgq.token_digest($1), 'hex')",
            &[&token.as_str()],
        )
        .unwrap()
        .get(0);

    let store = Store::open_with(&Target::Shared(dsn.clone()), Some(token.clone()))
        .expect("open")
        .0;
    store
        .publish(
            &address,
            &json!({"v": 1, "type": "note", "from": "cli/a@h", "to": address,
                    "message": "hi", "meta": {"token": token}}),
        )
        .unwrap();

    let queue = queue_for(&mut raw, &address).expect("registered");
    let stored: String = raw
        .query_one(
            &format!("SELECT message::text FROM pgmq.\"q_{queue}\" LIMIT 1"),
            &[],
        )
        .unwrap()
        .get(0);
    assert!(
        !stored.contains(&token),
        "the credential is not in the record"
    );
    assert!(!stored.contains(&digest), "nor anything that derives one");
    assert!(
        !stored.contains("\"token\""),
        "the field itself is discarded: {stored}"
    );

    // And nothing on the wire either: a reader obtains no write capability.
    let from_api = store
        .meta("protocol_version")
        .unwrap()
        .expect("a version is recorded");
    assert!(!from_api.contains(&token));
}

#[test]
fn a_write_outside_the_scopes_is_refused_and_changes_nothing() {
    let Some(dsn) = dsn() else {
        return;
    };
    let _serialised = exclusive(&dsn);
    let mut raw = client(&dsn);
    let token: String = raw
        .query_one(
            "SELECT msgq.register_identity($1, ARRAY['group/red'])",
            &[&format!("cli/red-{}", unique()).as_str()],
        )
        .unwrap()
        .get(0);
    let store = Store::open_with(&Target::Shared(dsn.clone()), Some(token))
        .expect("open")
        .0;
    let inside = format!("group/red/sub-{}", unique());
    let outside = format!("group/redder-{}", unique());

    // A differently-spelled form of an address inside the scope is still inside it: the bus
    // compares the canonical spelling.
    store
        .publish(
            &format!(" /{inside}/ "),
            &json!({"v": 1, "type": "note", "from": "cli/lier@h", "to": inside,
                    "message": "hi"}),
        )
        .unwrap();
    assert!(queue_for(&mut raw, &inside).is_some());

    let before = raw
        .query_one("SELECT COUNT(*) FROM msgq.address", &[])
        .unwrap()
        .get::<_, i64>(0);
    let refused = store.publish(
        &outside,
        &json!({"v": 1, "type": "note", "from": "cli/lier@h", "to": outside, "message": "hi"}),
    );
    let error = refused.expect_err("a write outside the scopes is refused");
    assert!(error.to_string().contains("outside the scopes"), "{error}");
    let after = raw
        .query_one("SELECT COUNT(*) FROM msgq.address", &[])
        .unwrap()
        .get::<_, i64>(0);
    assert_eq!(before, after, "a refused write leaves the bus unchanged");
    assert_eq!(queue_for(&mut raw, &outside), None);
}

#[test]
fn a_missing_or_expired_credential_is_named() {
    let Some(dsn) = dsn() else {
        return;
    };
    let _serialised = exclusive(&dsn);
    let mut raw = client(&dsn);
    let address = format!("group/cred-{}", unique());

    // Ensure the bus has an identity, so it is a bus that can authorize.
    raw.execute(
        "SELECT msgq.register_identity($1, ARRAY['group'])",
        &[&format!("cli/keeper-{}", unique()).as_str()],
    )
    .unwrap();
    let anonymous = Store::open_with(&Target::Shared(dsn.clone()), None)
        .expect("open")
        .0;
    let error = anonymous
        .publish(
            &address,
            &json!({"v": 1, "type": "note", "from": "cli/a@h", "to": address, "message": "hi"}),
        )
        .expect_err("a bus with identities refuses an anonymous write");
    assert!(error.to_string().contains("no credential"), "{error}");

    let expired: String = raw
        .query_one(
            "SELECT msgq.register_identity($1, ARRAY['group'], now() - interval '1 hour')",
            &[&format!("cli/expired-{}", unique()).as_str()],
        )
        .unwrap()
        .get(0);
    let store = Store::open_with(&Target::Shared(dsn.clone()), Some(expired))
        .expect("open")
        .0;
    let error = store
        .publish(
            &address,
            &json!({"v": 1, "type": "note", "from": "cli/a@h", "to": address, "message": "hi"}),
        )
        .expect_err("an expired credential is refused");
    assert!(error.to_string().contains("expired"), "{error}");
}

#[test]
fn a_runtime_role_cannot_write_a_stream_directly() {
    let Some(dsn) = dsn() else {
        return;
    };
    let _serialised = exclusive(&dsn);
    let mut raw = client(&dsn);
    raw.batch_execute(
        "DO $$ BEGIN
             IF NOT EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'msgq_runtime') THEN
                 CREATE ROLE msgq_runtime LOGIN;
             END IF;
         END $$;
         SELECT msgq.grant_runtime('msgq_runtime');",
    )
    .unwrap();

    // A queue to try against, and a credential the runtime role may use through the function.
    let address = format!("group/runtime-{}", unique());
    let token: String = raw
        .query_one(
            "SELECT msgq.register_identity($1, ARRAY['group'])",
            &[&format!("cli/runtime-{}", unique()).as_str()],
        )
        .unwrap()
        .get(0);
    let store = Store::open_with(&Target::Shared(dsn.clone()), Some(token.clone()))
        .expect("open")
        .0;
    store
        .publish(
            &address,
            &json!({"v": 1, "type": "note", "from": "cli/a@h", "to": address, "message": "hi"}),
        )
        .unwrap();
    let queue = queue_for(&mut raw, &address).expect("registered");

    let runtime_dsn = dsn.replace("//msgq@", "//msgq_runtime@");
    assert_ne!(runtime_dsn, dsn, "the fixture connects as a non-owner");
    let mut runtime = Client::connect(&runtime_dsn, NoTls).expect("the runtime role connects");

    let direct = runtime.execute(
        &format!("INSERT INTO pgmq.\"q_{queue}\" (message) VALUES ('{{\"v\":1}}'::jsonb)"),
        &[],
    );
    assert!(
        direct.is_err(),
        "the runtime role must not insert into a stream directly"
    );

    let created = runtime.execute("CREATE TABLE pgmq.sneaky ()", &[]);
    assert!(created.is_err(), "the runtime role must not create in pgmq");

    // And the one path it does have works, which is what makes the refusal meaningful.
    let offset: i64 = runtime
        .query_one(
            "SELECT msgq.publish_as($1, $2, $3::text::jsonb)",
            &[
                &token.as_str(),
                &address,
                &r#"{"v":1,"type":"note","message":"hi"}"#,
            ],
        )
        .unwrap()
        .get(0);
    assert!(offset > 0);
}

#[test]
fn the_reported_credential_state_distinguishes_the_four_cases() {
    let Some(dsn) = dsn() else {
        return;
    };
    let _serialised = exclusive(&dsn);
    let mut raw = client(&dsn);
    // Start from a bus that has no identity at all.
    raw.execute("DELETE FROM msgq.identity", &[]).unwrap();
    let anonymous = || {
        Store::open_with(&Target::Shared(dsn.clone()), None)
            .expect("open")
            .0
    };
    assert!(
        anonymous()
            .credential()
            .unwrap()
            .starts_with("none (no identity"),
        "a bus without identities says so"
    );

    let token = fixture_token(&dsn);
    assert!(
        anonymous()
            .credential()
            .unwrap()
            .starts_with("missing (this bus has"),
        "a bus that can authorize reports that a credential is required"
    );

    let known = Store::open_with(&Target::Shared(dsn.clone()), Some(token))
        .expect("open")
        .0;
    assert_eq!(known.credential().unwrap(), "accepted as cli/fixture");

    let unknown = Store::open_with(&Target::Shared(dsn.clone()), Some("nope".to_string()))
        .expect("open")
        .0;
    assert!(
        unknown.credential().unwrap().starts_with("rejected"),
        "an unknown credential is named as rejected, not as absent"
    );

    // And the local bus is never asked for one.
    let local = Store::open_with(
        &Target::Local(std::env::temp_dir().join(format!("msgq-cred-{}.db", unique()))),
        None,
    )
    .expect("open")
    .0;
    let described = local.credential().unwrap();
    assert!(described.contains("file permissions"), "{described}");
    assert!(
        described.contains("claim"),
        "no false assurance: {described}"
    );
}
