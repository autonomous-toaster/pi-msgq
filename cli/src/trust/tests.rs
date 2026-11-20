//! What a reader concludes about a record, checked against the anchors it was given.

use ed25519_dalek::SigningKey;
use serde_json::{Value, json};

use super::*;

fn record() -> Value {
    json!({
        "v": 1,
        "type": "prompt",
        "from": "session/alice-1",
        "to": "session/red",
        "message": "hello",
    })
}

fn signed(seed: u8) -> (Value, String) {
    let signing = SigningKey::from_bytes(&[seed; 32]);
    let mut value = record();
    keys::sign_value(&mut value, &signing).unwrap();
    let fingerprint = keys::fingerprint(&signing.verifying_key());
    (value, fingerprint)
}

/// A fingerprint that is well formed without being anybody's key.
fn hex_fingerprint(c: char) -> String {
    format!("SHA256:{}", c.to_string().repeat(64))
}

fn reader(anchors: Vec<Located>) -> Reader {
    Reader { anchors }
}

#[test]
fn a_signed_record_from_a_pinned_key_is_trusted() {
    let (value, fingerprint) = signed(5);
    let verdict = reader(vec![Located {
        anchor: Anchor::new("alice", fingerprint.clone()),
        layer: Layer::Durable,
    }])
    .verdict(&value);
    assert_eq!(
        verdict,
        Verdict::Trusted {
            principal: "alice".to_string(),
            fingerprint,
        }
    );
}

#[test]
fn a_stranger_is_unverified_not_refused() {
    let (value, fingerprint) = signed(5);
    let verdict = reader(Vec::new()).verdict(&value);
    assert_eq!(
        verdict,
        Verdict::Unverified {
            fingerprint: Some(fingerprint),
            reason: "no anchor for this key".to_string(),
        }
    );
}

#[test]
fn an_unsigned_record_is_unverified_and_says_unsigned() {
    let verdict = reader(Vec::new()).verdict(&record());
    assert!(verdict.is_unsigned());
    assert_eq!(verdict.state(), "unverified");
}

#[test]
fn a_tampered_record_is_refused_whatever_the_anchors_say() {
    let (mut value, fingerprint) = signed(5);
    value["message"] = json!("goodbye");
    let verdict = reader(vec![Located {
        anchor: Anchor::new("alice", fingerprint.clone()),
        layer: Layer::Durable,
    }])
    .verdict(&value);
    assert_eq!(verdict.state(), "refused");
    assert!(verdict.line().contains("refused"));
}

#[test]
fn a_retired_key_is_refused_rather_than_unverified() {
    let (value, fingerprint) = signed(5);
    let mut anchor = Anchor::new("alice", hex_fingerprint('a'));
    anchor.retired.push(Retired {
        fingerprint: fingerprint.clone(),
        retired_at: "2026-11-20".to_string(),
    });
    let verdict = reader(vec![Located {
        anchor,
        layer: Layer::Durable,
    }])
    .verdict(&value);
    assert_eq!(verdict.state(), "refused");
    assert!(verdict.line().contains("2026-11-20"), "{}", verdict.line());
}

#[test]
fn a_malformed_layer_entry_is_fatal() {
    assert!(parse_layer(TRUST_ENV, "alice", Layer::Environment).is_err());
    assert!(parse_layer(TRUST_ENV, "alice:", Layer::Environment).is_err());
    assert!(parse_layer(TRUST_ENV, ":SHA256:x", Layer::Environment).is_err());
}

#[test]
fn two_fingerprints_for_one_principal_are_fatal() {
    let two = format!(
        "alice:{},alice:{}",
        hex_fingerprint('a'),
        hex_fingerprint('b')
    );
    let error = parse_layer(TRUST_ENV, &two, Layer::Environment).unwrap_err();
    assert!(error.to_string().contains("two fingerprints"), "{error}");
}

#[test]
fn the_same_fingerprint_twice_collapses() {
    let one = format!("alice:{}", hex_fingerprint('a'));
    let twice = format!("{one},{one}");
    assert_eq!(
        parse_layer(TRUST_ENV, &twice, Layer::Environment)
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn a_bus_name_is_stable_and_differs_between_buses() {
    let a = Target::Local(PathBuf::from("/tmp/one.db"));
    let b = Target::Local(PathBuf::from("/tmp/two.db"));
    assert_eq!(bus_id(&a).unwrap(), bus_id(&a).unwrap());
    assert_ne!(bus_id(&a).unwrap(), bus_id(&b).unwrap());

    // Two spellings of one path are one bus, or an anchor would stop applying after a cd.
    let spelled_differently = Target::Local(PathBuf::from("/tmp/./one.db"));
    assert_eq!(bus_id(&a).unwrap(), bus_id(&spelled_differently).unwrap());
}

#[test]
fn several_anchors_can_be_given_at_once() {
    let two = format!(
        "alice:{},bob:{}",
        hex_fingerprint('a'),
        hex_fingerprint('b')
    );
    let parsed = parse_layer(TRUST_ENV, &two, Layer::Environment).unwrap();
    assert_eq!(parsed.len(), 2);
    assert_eq!(parsed[0].anchor.principal, "alice");
    assert_eq!(parsed[1].anchor.principal, "bob");
    assert!(parsed.iter().all(|l| l.layer == Layer::Environment));

    // A shell may quote the value with spaces instead of commas.
    let spaced = format!(
        "alice:{} bob:{}",
        hex_fingerprint('a'),
        hex_fingerprint('b')
    );
    assert_eq!(
        parse_layer(TRUST_ENV, &spaced, Layer::Environment)
            .unwrap()
            .len(),
        2
    );
}

#[test]
fn a_password_never_reaches_a_directory_name() {
    let dsn = "postgres://alice:hunter2@db.example:5432/msgq";
    let target = Target::Shared(dsn.to_string());
    let id = bus_id(&target).unwrap();
    assert!(!id.contains("hunter2"), "{id}");
    assert!(id.contains("db_example"), "{id}");
}
