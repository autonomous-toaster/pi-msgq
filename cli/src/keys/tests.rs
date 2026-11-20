//! What a key is, what it covers, and what it must never reveal.

use super::*;
use serde_json::json;

fn record() -> Value {
    json!({
        "v": 1,
        "type": "message",
        "from": "session/alice-1",
        "to": "group/red",
        "message": "hello",
    })
}

#[test]
fn a_claim_survives_a_signature() {
    let signing = SigningKey::from_bytes(&[7u8; 32]);
    let mut value = record();
    sign_value(&mut value, &signing).unwrap();
    let claim = claim(&value).expect("a claim");
    assert_eq!(claim.fingerprint, fingerprint(&signing.verifying_key()));
    assert!(verify_claim(&value, &claim).is_ok());
}

#[test]
fn a_tampered_body_fails() {
    let signing = SigningKey::from_bytes(&[7u8; 32]);
    let mut value = record();
    sign_value(&mut value, &signing).unwrap();
    let claim = claim(&value).unwrap();
    value["message"] = json!("goodbye");
    assert!(verify_claim(&value, &claim).is_err());
}

#[test]
fn a_restamped_sender_fails() {
    let signing = SigningKey::from_bytes(&[7u8; 32]);
    let mut value = record();
    sign_value(&mut value, &signing).unwrap();
    let claim = claim(&value).unwrap();
    value["from"] = json!("session/mallory");
    assert!(verify_claim(&value, &claim).is_err());
}

#[test]
fn a_moved_address_fails() {
    let signing = SigningKey::from_bytes(&[7u8; 32]);
    let mut value = record();
    sign_value(&mut value, &signing).unwrap();
    let claim = claim(&value).unwrap();
    value["to"] = json!("group/blue");
    assert!(verify_claim(&value, &claim).is_err());
}

#[test]
fn two_spellings_of_one_sender_sign_the_same_bytes() {
    let plain = record();
    let mut other = record();
    other["from"] = json!("  session/alice-1  ");
    other["to"] = json!("red");
    assert_eq!(signed_bytes(&plain).unwrap(), signed_bytes(&other).unwrap());
}

#[test]
fn a_field_cannot_shift_into_its_neighbour() {
    let mut a = record();
    a["from"] = json!("session/a");
    a["to"] = json!("group/bc");
    let mut b = record();
    b["from"] = json!("session/ab");
    b["to"] = json!("group/c");
    assert_ne!(signed_bytes(&a).unwrap(), signed_bytes(&b).unwrap());
}

#[test]
fn an_unknown_algorithm_is_no_claim() {
    let signing = SigningKey::from_bytes(&[7u8; 32]);
    let mut value = record();
    sign_value(&mut value, &signing).unwrap();
    value["meta"]["sig"]["alg"] = json!("something-else");
    assert!(claim(&value).is_none());
}

#[test]
fn an_unsigned_record_is_no_claim() {
    assert!(claim(&record()).is_none());
}

#[test]
fn a_fingerprint_is_comparable_in_three_spellings() {
    let signing = SigningKey::from_bytes(&[3u8; 32]);
    let canonical = fingerprint(&signing.verifying_key());
    let bare = canonical.strip_prefix("SHA256:").unwrap().to_string();
    let hex: String = Sha256::digest(signing.verifying_key().to_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    assert_eq!(normalize_fingerprint(&canonical).unwrap(), canonical);
    assert_eq!(normalize_fingerprint(&bare).unwrap(), canonical);
    assert_eq!(normalize_fingerprint(&hex).unwrap(), canonical);
}

/// The pinned public key of the fixture key, so the fixtures are byte vectors and not prose.
const FIXTURE_KEY: &str = "GX9rI+FshTLGq8g4+s1ep4m+DHaykgM0A5v6iz02jWE";

fn fixture(name: &str) -> Value {
    let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../protocol/fixtures")
        .join(name);
    let raw = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    serde_json::from_str(&raw).expect("a fixture is JSON")
}

#[test]
fn the_signed_fixture_verifies_against_its_pinned_key() {
    let value = fixture("17-signed-record.json");
    let claim = claim(&value).expect("the fixture carries a claim");
    assert_eq!(claim.key, parse_public(FIXTURE_KEY).unwrap());
    assert!(verify_claim(&value, &claim).is_ok());
}

#[test]
fn the_tampered_fixture_fails_against_the_same_key() {
    let value = fixture("19-signature-tampered.json");
    let claim = claim(&value).expect("the fixture carries a claim");
    assert!(verify_claim(&value, &claim).is_err());
}

#[test]
fn the_unknown_algorithm_fixture_is_no_claim() {
    let value = fixture("18-signature-unknown-alg.json");
    assert!(claim(&value).is_none());
}

#[test]
fn the_agreement_key_is_derived_and_not_the_signing_seed() {
    let signing = SigningKey::from_bytes(&[9u8; 32]);
    let first = agreement_secret(&signing).to_bytes();
    let again = agreement_secret(&signing).to_bytes();
    assert_eq!(first, again, "the derivation is stable");
    assert_ne!(first, signing.to_bytes(), "it is not the seed itself");
    assert_ne!(
        first,
        agreement_secret(&SigningKey::from_bytes(&[10u8; 32])).to_bytes(),
        "two principals agree different keys"
    );
    // The public half is what travels, and it is not the signing public key.
    assert_ne!(
        agreement_public(&signing).to_bytes(),
        signing.verifying_key().to_bytes()
    );
}

#[test]
fn a_signed_record_advertises_its_agreement_key_under_the_signature() {
    let signing = SigningKey::from_bytes(&[11u8; 32]);
    let mut value = record();
    sign_value(&mut value, &signing).unwrap();

    let claim = claim(&value).expect("a claim");
    assert_eq!(
        claim.agreement.map(|k| k.to_bytes()),
        Some(agreement_public(&signing).to_bytes())
    );
    assert!(verify_claim(&value, &claim).is_ok());
}

#[test]
fn substituting_the_agreement_key_breaks_the_signature() {
    let signing = SigningKey::from_bytes(&[12u8; 32]);
    let mut value = record();
    sign_value(&mut value, &signing).unwrap();

    // Another principal's agreement key, swapped in by whoever stores the record.
    let other = agreement_public(&SigningKey::from_bytes(&[13u8; 32]));
    value["meta"]["sig"]["agree"] =
        serde_json::Value::String(STANDARD_NO_PAD.encode(other.to_bytes()));

    let claim = claim(&value).expect("a claim");
    assert!(
        verify_claim(&value, &claim).is_err(),
        "the signature covers the agreement key"
    );
}

#[test]
fn a_record_signed_before_this_change_still_verifies() {
    // The pinned fixture was signed without an agreement key, under the older domain.
    let value = fixture("17-signed-record.json");
    assert!(value["meta"]["sig"].get("agree").is_none());
    let claim = claim(&value).expect("the fixture carries a claim");
    assert!(claim.agreement.is_none());
    assert!(verify_claim(&value, &claim).is_ok());
}

#[test]
fn a_principal_name_is_not_a_path() {
    assert!(key_path("alice").is_ok());
    assert!(key_path("session/alice").is_err());
    assert!(key_path("").is_err());
    assert!(key_path("..").is_err());
}
