//! Sealing a body to recipients.
//!
//! The passphrase scheme seals a body for whoever holds one key. This one seals it for named people:
//! a content key per record, wrapped once per recipient under a key agreed with that recipient, each
//! wrap with its own ephemeral. The store still never sees a key and never re-encrypts anything, so
//! both engines behave identically — a sealed body is a string to them.
//!
//! What travels: the algorithm, the additional-data version, the body nonce, and one entry per
//! recipient naming the address it is for, the fingerprint of the key that opens it, that wrap's
//! ephemeral public key, its nonce, and the wrapped content key. No private material, ever.

use aes_gcm::aead::{Aead, KeyInit, Payload};
use aes_gcm::{Aes256Gcm, Nonce};
use base64::Engine as _;
use base64::engine::general_purpose::{STANDARD, STANDARD_NO_PAD};
use ed25519_dalek::SigningKey;
use hkdf::Hkdf;
use rand::RngCore;
use serde_json::Value;
use sha2::{Digest, Sha256};
use x25519_dalek::{PublicKey, StaticSecret};

use crate::address;
use crate::error::Error;
use crate::keys;

/// The algorithm tag of an envelope. A reader switches on this, so an older seal keeps its own tag.
pub const ALG_ENVELOPE: &str = "x25519+aes-256-gcm";

/// The additional-data version this scheme binds. An absent version means the original one.
pub const AAD_V2: &str = "v2";

/// The label the wrapping key is derived under, so it never equals a key used for anything else.
const WRAP_INFO: &[u8] = b"msgq/seal/v1";

/// One party a record is sealed for.
pub struct Recipient {
    /// The address this wrap is for, for a human reading the record later.
    pub address: String,
    /// The agreement public key that opens it.
    pub key: PublicKey,
}

impl Recipient {
    /// A recipient for this machine's own session: its address and its own agreement key.
    pub fn this_machine(address: &str, signing: &SigningKey) -> Self {
        Recipient {
            address: address.to_string(),
            key: keys::agreement_public(signing),
        }
    }
}

/// What a reader concluded about a sealed record.
pub enum Opened {
    /// The body was opened in place.
    Body,
    /// The record is sealed, and for parties this reader is not one of.
    Elsewhere(Vec<String>),
    /// The record names an algorithm this build does not implement.
    UnknownAlgorithm(String),
}

/// Whether a record declares an envelope this module seals.
pub fn is_envelope(value: &Value) -> bool {
    value
        .get("meta")
        .and_then(|meta| meta.get("enc"))
        .and_then(|enc| enc.get("alg"))
        .and_then(Value::as_str)
        == Some(ALG_ENVELOPE)
}

/// The fingerprint that names an agreement key, and the form a recipient entry carries.
pub fn agreement_fingerprint(key: &PublicKey) -> String {
    let digest = Sha256::digest(key.to_bytes());
    format!("SHA256:{}", STANDARD_NO_PAD.encode(digest))
}

/// What a body in `value` is bound to, under the version the record declares.
///
/// Version two adds the route: the canonical sender and the address. It can, because the sender is
/// inside the signed bytes and is verified before a body is opened, so binding it no longer makes a
/// record unreadable the moment authorization touches it.
pub fn additional_data(value: &Value) -> Result<Vec<u8>, Error> {
    let version = value.get("v").and_then(Value::as_i64).unwrap_or(0);
    let kind = value.get("type").and_then(Value::as_str).unwrap_or("");
    let enc = value.get("meta").and_then(|meta| meta.get("enc"));
    let declared = enc.and_then(|enc| enc.get("aad")).and_then(Value::as_str);
    if declared != Some(AAD_V2) {
        return Ok(format!("msgq/{version}/{kind}").into_bytes());
    }

    let from = value
        .get("from")
        .and_then(Value::as_str)
        .ok_or_else(|| Error::runtime("a body bound to its route needs a sender"))?;
    let to = value
        .get("to")
        .and_then(Value::as_str)
        .ok_or_else(|| Error::runtime("a body bound to its route needs an address"))?;
    let mut out = format!("msgq/{version}/{kind}\0").into_bytes();
    out.extend_from_slice(address::canonicalize(from)?.as_bytes());
    out.push(0);
    out.extend_from_slice(address::canonicalize(to)?.as_bytes());
    Ok(out)
}

/// Seal the body in place for every recipient. Returns false when the record has no body to seal.
pub fn seal_for(value: &mut Value, recipients: &[Recipient]) -> Result<bool, Error> {
    let Some(field) = sealable_field(value) else {
        return Ok(false);
    };
    let plaintext = plaintext_of(value, field)?;

    // The declaration has to be in place before the additional data is computed: the version lives in it.
    let mut enc = serde_json::json!({
        "alg": ALG_ENVELOPE,
        "aad": AAD_V2,
        "field": field,
    });
    set_enc(value, enc.clone());
    let aad = additional_data(value)?;

    // One content key per record, so the body is sealed once however many people read it.
    let mut cek_bytes = [0u8; 32];
    rand::rngs::OsRng.fill_bytes(&mut cek_bytes);
    let (body_nonce, body) = cipher_seal(&cek_bytes, &plaintext, &aad)?;

    let mut wraps: Vec<Value> = Vec::new();
    for recipient in recipients {
        // A fresh ephemeral per recipient: two readers must not be able to confirm they received the
        // same ciphertext by comparing one public value.
        let mut scalar = [0u8; 32];
        rand::rngs::OsRng.fill_bytes(&mut scalar);
        let ephemeral = StaticSecret::from(scalar);
        let ephemeral_public = PublicKey::from(&ephemeral);
        let shared = ephemeral.diffie_hellman(&recipient.key);
        let kek = wrap_key(shared.as_bytes(), &ephemeral_public, &recipient.key);
        let (wrap_nonce, boxed) = cipher_seal(&kek, &cek_bytes, &aad)?;
        wraps.push(serde_json::json!({
            "to": recipient.address,
            "fp": agreement_fingerprint(&recipient.key),
            "epk": STANDARD_NO_PAD.encode(ephemeral_public.to_bytes()),
            "nonce": wrap_nonce,
            "box": boxed,
        }));
    }

    enc["nonce"] = Value::String(body_nonce);
    enc["cek"] = Value::Array(wraps);
    set_enc(value, enc);
    value[field] = Value::String(body);
    Ok(true)
}

/// Open the body in place, if this reader is one of the recipients.
pub fn open_for(value: &mut Value, signing: &SigningKey) -> Result<Opened, Error> {
    let enc = value
        .get("meta")
        .and_then(|meta| meta.get("enc"))
        .cloned()
        .ok_or_else(|| Error::runtime("the record declares no encoding"))?;
    let alg = enc.get("alg").and_then(Value::as_str).unwrap_or("");
    if alg != ALG_ENVELOPE {
        return Ok(Opened::UnknownAlgorithm(alg.to_string()));
    }

    let mine = agreement_fingerprint(&keys::agreement_public(signing));
    let wraps = enc
        .get("cek")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let Some(wrap) = wraps
        .iter()
        .find(|w| w.get("fp").and_then(Value::as_str) == Some(mine.as_str()))
    else {
        // Sealed, but not for this reader: a fact about the record, not a failure of the read.
        let for_whom: Vec<String> = wraps
            .iter()
            .filter_map(|w| {
                w.get("to")
                    .and_then(Value::as_str)
                    .map(str::to_string)
                    .or_else(|| w.get("fp").and_then(Value::as_str).map(str::to_string))
            })
            .collect();
        return Ok(Opened::Elsewhere(for_whom));
    };

    let ephemeral_public = wrap
        .get("epk")
        .and_then(Value::as_str)
        .and_then(keys::parse_agreement_public)
        .ok_or_else(|| Error::runtime("the record carries no usable ephemeral key"))?;
    let secret = keys::agreement_secret(signing);
    let shared = secret.diffie_hellman(&ephemeral_public);
    let kek = wrap_key(
        shared.as_bytes(),
        &ephemeral_public,
        &keys::agreement_public(signing),
    );

    let aad = additional_data(value)?;
    let wrap_nonce = wrap
        .get("nonce")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let boxed = wrap.get("box").and_then(Value::as_str).unwrap_or_default();
    let cek = cipher_open(&kek, wrap_nonce, boxed, &aad)
        .map_err(|_| Error::runtime("the content key does not open for this reader"))?;

    let field = enc
        .get("field")
        .and_then(Value::as_str)
        .unwrap_or("message");
    let body = value
        .get(field)
        .and_then(Value::as_str)
        .ok_or_else(|| Error::runtime(format!("the record carries no {field} body")))?;
    let nonce = enc.get("nonce").and_then(Value::as_str).unwrap_or_default();
    let plaintext = cipher_open(&cek, nonce, body, &aad)
        .map_err(|_| Error::runtime("the body does not open with this key"))?;

    if field == "message" {
        let text = String::from_utf8(plaintext)
            .map_err(|_| Error::runtime("the opened body is not text"))?;
        value["message"] = Value::String(text);
    } else {
        let decoded: Value = serde_json::from_slice(&plaintext)
            .map_err(|error| Error::runtime(format!("the opened body is not JSON: {error}")))?;
        value["event"] = decoded;
    }
    // The declaration is honoured, so it is dropped from the reader's copy.
    if let Some(meta) = value.get_mut("meta").and_then(Value::as_object_mut) {
        meta.remove("enc");
    }
    Ok(Opened::Body)
}

/// Which field holds the body, if this record has one that may be sealed.
fn sealable_field(value: &Value) -> Option<&'static str> {
    if !crate::record::body_may_be_opaque(value) {
        return None;
    }
    if value.get("message").and_then(Value::as_str).is_some() {
        return Some("message");
    }
    if value.get("event").map(Value::is_object).unwrap_or(false) {
        return Some("event");
    }
    None
}

fn plaintext_of(value: &Value, field: &str) -> Result<Vec<u8>, Error> {
    if field == "message" {
        return Ok(value
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .as_bytes()
            .to_vec());
    }
    Ok(serde_json::to_vec(
        value.get("event").unwrap_or(&Value::Null),
    )?)
}

fn set_enc(value: &mut Value, enc: Value) {
    match value.get_mut("meta").and_then(Value::as_object_mut) {
        Some(meta) => {
            meta.insert("enc".to_string(), enc);
        }
        None => value["meta"] = serde_json::json!({ "enc": enc }),
    }
}

/// The key one wrap uses: agreed with the recipient, bound to the pair so it is unique to them.
fn wrap_key(shared: &[u8; 32], ephemeral: &PublicKey, recipient: &PublicKey) -> [u8; 32] {
    let mut salt = Vec::with_capacity(64);
    salt.extend_from_slice(ephemeral.as_bytes());
    salt.extend_from_slice(recipient.as_bytes());
    let hk = Hkdf::<Sha256>::new(Some(&salt), shared);
    let mut out = [0u8; 32];
    hk.expand(WRAP_INFO, &mut out)
        .expect("32 bytes is a valid HKDF output length");
    out
}

fn cipher_seal(key: &[u8; 32], plaintext: &[u8], aad: &[u8]) -> Result<(String, String), Error> {
    let cipher = Aes256Gcm::new_from_slice(key)
        .map_err(|error| Error::runtime(format!("cannot initialise the cipher: {error}")))?;
    let mut nonce = [0u8; 12];
    rand::rngs::OsRng.fill_bytes(&mut nonce);
    let sealed = cipher
        .encrypt(
            Nonce::from_slice(&nonce),
            Payload {
                msg: plaintext,
                aad,
            },
        )
        .map_err(|_| Error::runtime("cannot seal the body"))?;
    Ok((STANDARD.encode(nonce), STANDARD.encode(sealed)))
}

fn cipher_open(key: &[u8], nonce: &str, body: &str, aad: &[u8]) -> Result<Vec<u8>, Error> {
    let nonce = STANDARD
        .decode(nonce)
        .map_err(|_| Error::runtime("the record carries no usable nonce"))?;
    if nonce.len() != 12 {
        return Err(Error::runtime("the record carries no usable nonce"));
    }
    let body = STANDARD
        .decode(body)
        .map_err(|_| Error::runtime("the body is not base64"))?;
    let cipher = Aes256Gcm::new_from_slice(key)
        .map_err(|error| Error::runtime(format!("cannot initialise the cipher: {error}")))?;
    cipher
        .decrypt(Nonce::from_slice(&nonce), Payload { msg: &body, aad })
        .map_err(|_| Error::runtime("the body does not open with this key"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::SigningKey;
    use serde_json::json;

    fn record() -> Value {
        json!({
            "v": 1,
            "type": "prompt",
            "from": "session/sender",
            "to": "session/reader",
            "message": "the plan is tomorrow",
        })
    }

    fn recipient(address: &str, seed: u8) -> (Recipient, SigningKey) {
        let signing = SigningKey::from_bytes(&[seed; 32]);
        (
            Recipient {
                address: address.to_string(),
                key: keys::agreement_public(&signing),
            },
            signing,
        )
    }

    #[test]
    fn two_seals_of_one_body_differ() {
        let (one, _) = recipient("session/a", 1);
        let mut first = record();
        let mut second = record();
        assert!(seal_for(&mut first, std::slice::from_ref(&one)).unwrap());
        assert!(seal_for(&mut second, std::slice::from_ref(&one)).unwrap());
        assert_ne!(first["message"], second["message"]);
        assert_ne!(
            first["meta"]["enc"]["nonce"],
            second["meta"]["enc"]["nonce"]
        );
    }

    #[test]
    fn each_recipient_opens_the_body_and_nobody_else_does() {
        let (reader, reader_key) = recipient("session/reader", 2);
        let (sender, sender_key) = recipient("session/sender", 3);
        let (_, stranger_key) = recipient("session/stranger", 4);

        let mut value = record();
        seal_for(&mut value, &[reader, sender]).unwrap();

        let mut mine = value.clone();
        assert!(matches!(
            open_for(&mut mine, &reader_key).unwrap(),
            Opened::Body
        ));
        assert_eq!(mine["message"], "the plan is tomorrow");
        // Honouring the declaration leaves nothing claiming to be encoded.
        assert!(mine["meta"].get("enc").is_none());

        // The sender can read back what it sent.
        let mut sent = value.clone();
        assert!(matches!(
            open_for(&mut sent, &sender_key).unwrap(),
            Opened::Body
        ));
        assert_eq!(sent["message"], "the plan is tomorrow");

        // A third party is told who it is for, and that is not an error.
        let mut stranger = value.clone();
        match open_for(&mut stranger, &stranger_key).unwrap() {
            Opened::Elsewhere(for_whom) => {
                assert!(
                    for_whom.contains(&"session/reader".to_string()),
                    "{for_whom:?}"
                );
                assert!(
                    for_whom.contains(&"session/sender".to_string()),
                    "{for_whom:?}"
                );
            }
            _ => panic!("a record sealed to others is not this reader's"),
        }
        // And nothing was opened or blanked in the copy it was given.
        assert_eq!(stranger["message"], value["message"]);
    }

    #[test]
    fn each_wrap_uses_its_own_ephemeral() {
        let (one, _) = recipient("session/a", 5);
        let (two, _) = recipient("session/b", 6);
        let mut value = record();
        seal_for(&mut value, &[one, two]).unwrap();
        let wraps = value["meta"]["enc"]["cek"].as_array().unwrap();
        assert_eq!(wraps.len(), 2);
        assert_ne!(wraps[0]["epk"], wraps[1]["epk"]);
        assert_ne!(wraps[0]["fp"], wraps[1]["fp"]);
    }

    #[test]
    fn a_body_cannot_be_moved_to_another_address() {
        let (reader, reader_key) = recipient("session/reader", 7);
        let mut value = record();
        seal_for(&mut value, &[reader]).unwrap();

        // Whoever stores the record re-addresses it: the route is inside the additional data.
        value["to"] = json!("session/somebody-else");
        let mut moved = value.clone();
        assert!(
            open_for(&mut moved, &reader_key).is_err(),
            "a body bound to its route does not open elsewhere"
        );
    }

    #[test]
    fn a_tampered_wrap_or_body_is_an_error() {
        let (reader, reader_key) = recipient("session/reader", 8);

        let mut wrapped = record();
        seal_for(&mut wrapped, &[reader]).unwrap();
        let mut altered_wrap = wrapped.clone();
        altered_wrap["meta"]["enc"]["cek"][0]["box"] = json!("AAAA");
        assert!(open_for(&mut altered_wrap, &reader_key).is_err());

        let (reader2, reader_key2) = recipient("session/reader", 8);
        let mut body = record();
        seal_for(&mut body, &[reader2]).unwrap();
        body["message"] = json!("AAAA");
        assert!(open_for(&mut body, &reader_key2).is_err());
    }

    #[test]
    fn an_unknown_algorithm_is_reported_not_failed() {
        let (_, reader_key) = recipient("session/reader", 9);
        let mut value = record();
        value["meta"] = json!({ "enc": { "alg": "future-cipher", "kid": "x" } });
        match open_for(&mut value, &reader_key).unwrap() {
            Opened::UnknownAlgorithm(alg) => assert_eq!(alg, "future-cipher"),
            _ => panic!("an algorithm this build lacks is a report, not an error"),
        }
    }

    #[test]
    fn version_one_records_keep_the_older_additional_data() {
        // No declaration, or a declaration without a version, means the original binding.
        let plain = record();
        assert_eq!(additional_data(&plain).unwrap(), b"msgq/1/prompt".to_vec());
        let mut declared = record();
        declared["meta"] = json!({ "enc": { "alg": "aes-256-gcm", "kid": "abc" } });
        assert_eq!(
            additional_data(&declared).unwrap(),
            b"msgq/1/prompt".to_vec()
        );

        // And version two binds the route.
        let (reader, _) = recipient("session/reader", 10);
        let mut sealed = record();
        seal_for(&mut sealed, &[reader]).unwrap();
        let aad = additional_data(&sealed).unwrap();
        assert!(aad.starts_with(b"msgq/1/prompt\0session/sender\0session/reader"));
    }

    #[test]
    fn a_record_with_no_sealable_body_is_left_alone() {
        let (reader, _) = recipient("session/reader", 11);
        let mut emptyless = json!({
            "v": 1, "type": "event",
            "from": "session/a", "to": "session/b",
            "event": { "kind": "ack", "phase": "received" }
        });
        // Acknowledgements and presence are never sealed: the lifecycle reads them.
        assert!(!seal_for(&mut emptyless, &[reader]).unwrap());
        assert!(emptyless.get("meta").is_none());
    }
}
