//! Sealing a record body.
//!
//! One authenticated cipher per principal, keyed from a passphrase. The store never sees a key and
//! never re-encrypts anything: a sealed body is a string to the engine, which is why the local and
//! shared engines behave identically.
//!
//! What travels in the record is the algorithm, a key identifier, a nonce, and the field the body
//! came from. Nothing that derives the key travels at all.

use aes_gcm::aead::{Aead, KeyInit, Payload};
use aes_gcm::{Aes256Gcm, Nonce};
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64;
use rand::RngCore;
use scrypt::Params;
use serde_json::Value;

use crate::record::body_may_be_opaque;
use sha2::{Digest, Sha256};

use crate::error::Error;

/// The authenticated cipher this build uses.
pub const ALG: &str = "aes-256-gcm";

/// Where the passphrase comes from. The environment first, then a file, so an agent can be handed
/// the secret without putting it in a command line.
pub const PASSPHRASE_ENV: &str = "MSGQ_PASSPHRASE";
pub const PASSPHRASE_FILE_ENV: &str = "MSGQ_PASSPHRASE_FILE";

/// A deployment-wide salt, so two buses do not share a key derivation unless the operator chooses
/// to. The salt is not a secret; it is what stops one table of derived keys serving every bus.
pub const SALT_ENV: &str = "MSGQ_KEY_SALT";
pub const DEFAULT_SALT: &str = "msgq-encryption/v1";

/// Cost of the key derivation. Slow enough to make a guessed passphrase expensive, fast enough that
/// one derivation per command is unnoticeable.
const LOG_N: u8 = 15;

/// A derived key, its identifier, and where the passphrase came from.
pub struct Key {
    kid: String,
    bytes: [u8; 32],
    source: String,
}

impl Key {
    /// The configured key, or nothing when no passphrase is configured.
    ///
    /// A configured source that cannot be read is an error, not a silent fall back to plaintext: a
    /// deployment that meant to encrypt and did not is the failure this refuses.
    pub fn from_env() -> Result<Option<Self>, Error> {
        if let Ok(passphrase) = std::env::var(PASSPHRASE_ENV) {
            let passphrase = passphrase.trim().to_string();
            if !passphrase.is_empty() {
                return Ok(Some(Self::derive(&passphrase, &salt())?));
            }
        }
        if let Ok(path) = std::env::var(PASSPHRASE_FILE_ENV) {
            let passphrase = std::fs::read_to_string(&path).map_err(|error| {
                Error::usage(format!(
                    "{PASSPHRASE_FILE_ENV} names {path} but it cannot be read: {error}"
                ))
            })?;
            let passphrase = passphrase.trim().to_string();
            if passphrase.is_empty() {
                return Err(Error::usage(format!(
                    "{PASSPHRASE_FILE_ENV} names {path} but the file is empty"
                )));
            }
            let mut key = Self::derive(&passphrase, &salt())?;
            key.source = format!("file {path}");
            return Ok(Some(key));
        }
        Ok(None)
    }

    /// Derive the key from a passphrase and a salt.
    pub fn derive(passphrase: &str, salt: &str) -> Result<Self, Error> {
        let params = Params::new(LOG_N, 8, 1, 32).map_err(|error| {
            Error::runtime(format!("cannot configure the key derivation: {error}"))
        })?;
        let mut bytes = [0u8; 32];
        scrypt::scrypt(passphrase.as_bytes(), salt.as_bytes(), &params, &mut bytes)
            .map_err(|error| Error::runtime(format!("cannot derive a key: {error}")))?;
        Ok(Self {
            kid: kid_of(salt),
            bytes,
            source: "environment".to_string(),
        })
    }

    /// The key identifier: a name for the salt, never for the key.
    pub fn kid(&self) -> &str {
        &self.kid
    }

    /// Where the passphrase came from, for reporting.
    pub fn source(&self) -> &str {
        &self.source
    }

    /// Seal one body. Returns the nonce and the ciphertext, both base64.
    pub fn seal(&self, plaintext: &[u8], aad: &[u8]) -> Result<(String, String), Error> {
        let cipher = Aes256Gcm::new_from_slice(&self.bytes)
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
        Ok((BASE64.encode(nonce), BASE64.encode(sealed)))
    }

    /// Open one body. A body that was altered fails here instead of being read as content.
    pub fn open(&self, nonce: &str, body: &str, aad: &[u8]) -> Result<Vec<u8>, Error> {
        let nonce = BASE64
            .decode(nonce)
            .map_err(|_| Error::runtime("the record carries no usable nonce"))?;
        if nonce.len() != 12 {
            return Err(Error::runtime("the record carries no usable nonce"));
        }
        let body = BASE64
            .decode(body)
            .map_err(|_| Error::runtime("the body is not base64"))?;
        let cipher = Aes256Gcm::new_from_slice(&self.bytes)
            .map_err(|error| Error::runtime(format!("cannot initialise the cipher: {error}")))?;
        cipher
            .decrypt(Nonce::from_slice(&nonce), Payload { msg: &body, aad })
            .map_err(|_| Error::runtime("the body does not open with this key"))
    }
}

/// The salt this deployment derives keys with.
fn salt() -> String {
    std::env::var(SALT_ENV)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| DEFAULT_SALT.to_string())
}

/// A stable name for a key generation. Hashing the salt keeps the name public.
fn kid_of(salt: &str) -> String {
    let digest = Sha256::digest(salt.as_bytes());
    digest
        .iter()
        .take(8)
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// The additional data a sealed body is bound to: the record's version and type.
///
/// The sender and the recipient are left out on purpose: the bus stamps the sender after the client
/// seals, so binding either would make a sealed record unreadable the moment authorization touched it.
pub fn additional_data(record: &Value) -> Vec<u8> {
    let version = record.get("v").and_then(Value::as_i64).unwrap_or(0);
    let kind = record.get("type").and_then(Value::as_str).unwrap_or("");
    format!("msgq/{version}/{kind}").into_bytes()
}

/// How this deployment encrypts, for reporting. Never prints the passphrase or the key.
pub fn describe() -> Result<String, Error> {
    match Key::from_env()? {
        None => Ok("none (bodies are plaintext)".to_string()),
        Some(key) => Ok(format!(
            "{ALG} (kid={}, source={})",
            key.kid(),
            key.source()
        )),
    }
}

/// Which field holds a body that may be sealed.
///
/// Presence and acknowledgement bodies are never sealed: the roster and the lifecycle read them, and
/// `PROTOCOL.md` calls both public metadata.
fn sealable_field(value: &Value) -> Option<&'static str> {
    if !body_may_be_opaque(value) {
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

/// The declaration of an encoded body: the algorithm and the key identifier.
pub fn enc_info(value: &Value) -> Option<(String, String)> {
    let enc = value.get("meta")?.get("enc")?;
    let alg = enc.get("alg").and_then(Value::as_str)?;
    let kid = enc.get("kid").and_then(Value::as_str).unwrap_or("unknown");
    Some((alg.to_string(), kid.to_string()))
}

/// Seal the body of a record, in place. Returns false when the record has no body to seal.
pub fn seal_body(value: &mut Value, key: &Key) -> Result<bool, Error> {
    let Some(field) = sealable_field(value) else {
        return Ok(false);
    };
    let plaintext = match field {
        "message" => value
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .as_bytes()
            .to_vec(),
        _ => serde_json::to_vec(value.get("event").unwrap_or(&Value::Null))?,
    };
    let aad = crate::seal::additional_data(value);
    let (nonce, body) = key.seal(&plaintext, &aad)?;
    let enc = serde_json::json!({
        "alg": crate::seal::ALG,
        "kid": key.kid(),
        "nonce": nonce,
        "field": field,
    });
    if let Some(meta) = value.get_mut("meta").and_then(Value::as_object_mut) {
        meta.insert("enc".to_string(), enc);
    } else {
        value["meta"] = serde_json::json!({ "enc": enc });
    }
    value[field] = Value::String(body);
    Ok(true)
}

/// Open a sealed body, in place. A body that does not open is an error, never content.
pub fn open_body(value: &mut Value, key: &Key) -> Result<(), Error> {
    let enc = value
        .get("meta")
        .and_then(|meta| meta.get("enc"))
        .ok_or_else(|| Error::runtime("the record declares no encoding"))?;
    let alg = enc.get("alg").and_then(Value::as_str).unwrap_or("");
    let kid = enc.get("kid").and_then(Value::as_str).unwrap_or("unknown");
    if alg != crate::seal::ALG {
        return Err(Error::runtime(format!(
            "the record is encoded with {alg}, which this build does not implement (kid={kid})"
        )));
    }
    let nonce = enc.get("nonce").and_then(Value::as_str).unwrap_or_default();
    let field = enc
        .get("field")
        .and_then(Value::as_str)
        .unwrap_or("message")
        .to_string();
    let body = value
        .get(&field)
        .and_then(Value::as_str)
        .ok_or_else(|| Error::runtime(format!("the record carries no {field} body")))?;
    let aad = crate::seal::additional_data(value);
    let plaintext = key
        .open(nonce, body, &aad)
        .map_err(|error| Error::runtime(format!("{error} (kid={kid})")))?;
    if field == "message" {
        let text = String::from_utf8(plaintext)
            .map_err(|_| Error::runtime("the opened body is not text"))?;
        value["message"] = Value::String(text);
    } else {
        let decoded: Value = serde_json::from_slice(&plaintext)
            .map_err(|error| Error::runtime(format!("the opened body is not JSON: {error}")))?;
        value["event"] = decoded;
    }
    // The declaration is honoured, so it is dropped from the reader's copy: a body that is now
    // readable must not still claim to be encoded.
    if let Some(meta) = value.get_mut("meta").and_then(Value::as_object_mut) {
        meta.remove("enc");
    }
    Ok(())
}

/// Remove a body that cannot be opened, and leave the declaration in place so a reader can say why.
pub fn blank_body(value: &mut Value) {
    let field = value
        .get("meta")
        .and_then(|meta| meta.get("enc"))
        .and_then(|enc| enc.get("field"))
        .and_then(Value::as_str)
        .unwrap_or("message")
        .to_string();
    if let Some(object) = value.as_object_mut() {
        object.remove(&field);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_sealed_body_opens_with_the_same_key() {
        let key = Key::derive("correct horse", DEFAULT_SALT).unwrap();
        let (nonce, body) = key.seal(b"the plan", b"msgq/1/note").unwrap();
        let opened = key.open(&nonce, &body, b"msgq/1/note").unwrap();
        assert_eq!(opened, b"the plan");
    }

    #[test]
    fn two_seals_differ_even_for_one_body() {
        let key = Key::derive("correct horse", DEFAULT_SALT).unwrap();
        let (nonce_a, body_a) = key.seal(b"same", b"msgq/1/note").unwrap();
        let (nonce_b, body_b) = key.seal(b"same", b"msgq/1/note").unwrap();
        assert_ne!(nonce_a, nonce_b);
        assert_ne!(body_a, body_b);
    }

    #[test]
    fn another_passphrase_does_not_open() {
        let key = Key::derive("correct horse", DEFAULT_SALT).unwrap();
        let other = Key::derive("battery staple", DEFAULT_SALT).unwrap();
        let (nonce, body) = key.seal(b"the plan", b"msgq/1/note").unwrap();
        assert!(other.open(&nonce, &body, b"msgq/1/note").is_err());
        assert_eq!(key.kid(), other.kid(), "the identifier names the salt");
    }

    #[test]
    fn an_altered_body_does_not_open() {
        let key = Key::derive("correct horse", DEFAULT_SALT).unwrap();
        let (nonce, body) = key.seal(b"the plan", b"msgq/1/note").unwrap();
        let mut bytes = BASE64.decode(&body).unwrap();
        bytes[0] ^= 0x01;
        assert!(
            key.open(&nonce, &BASE64.encode(bytes), b"msgq/1/note")
                .is_err()
        );
    }

    #[test]
    fn a_body_sealed_for_one_type_does_not_open_as_another() {
        let key = Key::derive("correct horse", DEFAULT_SALT).unwrap();
        let (nonce, body) = key.seal(b"the plan", b"msgq/1/note").unwrap();
        assert!(key.open(&nonce, &body, b"msgq/1/prompt").is_err());
    }

    #[test]
    fn the_identifier_names_the_salt_not_the_key() {
        assert_eq!(kid_of("one"), kid_of("one"));
        assert_ne!(kid_of("one"), kid_of("two"));
        assert_eq!(kid_of("one").len(), 16);
    }

    fn fixture(name: &str) -> Value {
        let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../protocol/fixtures")
            .join(name);
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
    }

    #[test]
    fn the_sealed_fixture_opens_with_the_test_passphrase() {
        // The passphrase is a fixture value, not a secret: both clients derive the same key from it.
        let key = Key::derive("correct horse", DEFAULT_SALT).unwrap();
        let mut value = fixture("16-sealed-body.json");
        assert_eq!(
            enc_info(&value).map(|(_, kid)| kid).as_deref(),
            Some(key.kid())
        );

        let other = Key::derive("battery staple", DEFAULT_SALT).unwrap();
        let error = open_body(&mut value.clone(), &other).expect_err("another key must not open");
        assert!(error.to_string().contains("kid="), "{error}");

        open_body(&mut value, &key).expect("the fixture opens");
        assert_eq!(value["message"], "the plan is tomorrow");
        assert!(
            value["meta"].get("enc").is_none(),
            "a body that is now readable must not still claim to be encoded"
        );
    }

    #[test]
    fn a_sealed_event_body_round_trips() {
        let key = Key::derive("correct horse", DEFAULT_SALT).unwrap();
        let mut value = serde_json::json!({
            "v": 1, "type": "event", "from": "a", "to": "b",
            "event": { "kind": "custom", "detail": "secret" }
        });
        assert!(seal_body(&mut value, &key).unwrap());
        assert!(value["event"].is_string());
        open_body(&mut value, &key).unwrap();
        assert_eq!(value["event"]["detail"], "secret");
    }

    #[test]
    fn presence_and_acknowledgement_bodies_are_never_sealed() {
        let key = Key::derive("correct horse", DEFAULT_SALT).unwrap();
        for kind in ["presence", "ack"] {
            let mut value = serde_json::json!({
                "v": 1, "type": "event", "from": "a", "to": "b", "event": { "kind": kind }
            });
            assert!(!seal_body(&mut value, &key).unwrap());
            assert!(value["event"].is_object());
        }
    }
}
