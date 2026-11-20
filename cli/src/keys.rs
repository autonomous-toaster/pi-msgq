//! Signing keys: where a private key lives, what a fingerprint is, and the bytes a signature covers.
//!
//! Deliberately no trust decisions: this module knows how to make a key and how to check a
//! signature against a key it is given. Deciding whether that key is believed belongs to
//! `trust`, and the bus is never asked.

use std::io::Write;
use std::path::{Path, PathBuf};

use base64::Engine;
use base64::engine::general_purpose::STANDARD_NO_PAD;
use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::address;
use crate::config;
use crate::error::Error;

/// The signature algorithm this build implements, as it appears in `meta.sig.alg`.
pub const ALG: &str = "ed25519";

/// Domain separation: a signature made for this purpose can never be replayed as another.
const DOMAIN: &[u8] = b"msgq.sig.v1";

/// Prefix on a stored private key, so a mistaken file is refused rather than used.
const KEY_PREFIX: &str = "msgq-ed25519:";

/// What was made, so a caller can report it without reading the key back.
pub struct Generated {
    pub fingerprint: String,
    pub public: String,
    pub path: PathBuf,
}

/// The principal this machine signs as when no bus names one: `user@host`.
///
/// It is the default sender with its address scheme removed, because a key names a principal rather
/// than an address — several of this machine's sessions speak as different senders under one key.
pub fn principal_name() -> String {
    let sender = crate::record::default_sender();
    match sender.split_once('/') {
        Some((_, rest)) if !rest.is_empty() => rest.to_string(),
        _ => sender,
    }
}

/// The key a record claims to be signed with, and the fingerprint that names it.
pub struct Claim {
    pub key: VerifyingKey,
    pub fingerprint: String,
}

/// The directory this user's key material lives in.
pub fn keys_dir() -> Result<PathBuf, Error> {
    let dir = config::state_dir()
        .ok_or_else(|| Error::runtime("cannot find a home directory for a signing key"))?;
    Ok(dir.join("keys"))
}

/// The file holding one principal's private key.
///
/// A principal is a name, not an address, so a name carrying `/` is refused rather than
/// turned into a path that leaves the key directory.
pub fn key_path(principal: &str) -> Result<PathBuf, Error> {
    let name = principal.trim();
    if name.is_empty() {
        return Err(Error::usage("a key needs a principal name"));
    }
    if name.contains('/') || name.contains('\0') || name == "." || name == ".." {
        return Err(Error::usage(format!(
            "'{principal}' is not a principal name; a principal carries no address scheme or path"
        )));
    }
    Ok(keys_dir()?.join(format!("{name}.key")))
}

/// Generate a key for a principal and write it with mode 0600.
pub fn generate(principal: &str) -> Result<Generated, Error> {
    let path = key_path(principal)?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| Error::runtime(format!("cannot create {}: {e}", parent.display())))?;
    }
    let mut bytes = [0u8; 32];
    getrandom(&mut bytes);
    let signing = SigningKey::from_bytes(&bytes);
    let encoded = format!("{KEY_PREFIX}{}\n", STANDARD_NO_PAD.encode(bytes));
    write_private(&path, encoded.as_bytes())?;
    Ok(Generated {
        fingerprint: fingerprint(&signing.verifying_key()),
        public: STANDARD_NO_PAD.encode(signing.verifying_key().to_bytes()),
        path,
    })
}

/// Fill bytes from the operating system's randomness.
///
/// `rand` is already a dependency, but a key seed is worth taking straight from the kernel
/// rather than through a generator whose version this build does not otherwise care about.
fn getrandom(bytes: &mut [u8]) {
    use std::io::Read;
    let mut f = std::fs::File::open("/dev/urandom").expect("a system source of randomness");
    f.read_exact(bytes).expect("random bytes");
}

/// The key to sign as, making one when the principal has none.
///
/// Making a key trusts nobody, so it needs no ceremony and no consent — which is what lets the strict
/// default be usable on a bus nobody has signed on before. The report of what was made is returned
/// rather than printed, so the caller decides where it goes.
pub fn load_or_generate(principal: &str) -> Result<(SigningKey, Option<Generated>), Error> {
    if let Some(key) = load(principal)? {
        return Ok((key, None));
    }
    let made = generate(principal)?;
    let key = load(principal)?.ok_or_else(|| {
        Error::runtime(format!(
            "the key for {principal} was written but cannot be read back"
        ))
    })?;
    Ok((key, Some(made)))
}

/// Load a principal's private key, if one exists.
pub fn load(principal: &str) -> Result<Option<SigningKey>, Error> {
    let path = key_path(principal)?;
    let Ok(raw) = std::fs::read_to_string(&path) else {
        return Ok(None);
    };
    let line = raw.trim();
    let encoded = line
        .strip_prefix(KEY_PREFIX)
        .ok_or_else(|| Error::runtime(format!("{} is not a msgq key", path.display())))?;
    let bytes = STANDARD_NO_PAD
        .decode(encoded)
        .map_err(|e| Error::runtime(format!("{} is damaged: {e}", path.display())))?;
    let seed: [u8; 32] = bytes
        .try_into()
        .map_err(|_| Error::runtime(format!("{} is damaged: wrong key length", path.display())))?;
    Ok(Some(SigningKey::from_bytes(&seed)))
}

/// `SHA256:` and the digest of the public key, without padding — the form `ssh-keygen -l` prints,
/// so a fingerprint can be read out and compared by people who already know that ritual.
pub fn fingerprint(key: &VerifyingKey) -> String {
    let digest = Sha256::digest(key.to_bytes());
    format!("SHA256:{}", STANDARD_NO_PAD.encode(digest))
}

/// The public half, as it travels in `meta.sig.key`.
pub fn public(key: &VerifyingKey) -> String {
    STANDARD_NO_PAD.encode(key.to_bytes())
}

/// Accept a fingerprint in the `SHA256:...` form, or bare base64, or bare hex of the digest.
pub fn normalize_fingerprint(raw: &str) -> Result<String, Error> {
    let raw = raw.trim();
    if raw.is_empty() {
        return Err(Error::usage("a fingerprint cannot be empty"));
    }
    let body = raw
        .strip_prefix("SHA256:")
        .or_else(|| raw.strip_prefix("sha256:"))
        .unwrap_or(raw);
    if let Ok(bytes) = STANDARD_NO_PAD.decode(body)
        && bytes.len() == 32
    {
        return Ok(format!("SHA256:{}", STANDARD_NO_PAD.encode(bytes)));
    }
    if body.len() == 64 && body.chars().all(|c| c.is_ascii_hexdigit()) {
        let mut out = Vec::with_capacity(32);
        for i in (0..64).step_by(2) {
            let byte = u8::from_str_radix(&body[i..i + 2], 16)
                .map_err(|_| Error::usage(format!("'{raw}' is not a fingerprint")))?;
            out.push(byte);
        }
        return Ok(format!("SHA256:{}", STANDARD_NO_PAD.encode(out)));
    }
    Err(Error::usage(format!(
        "'{raw}' is not a fingerprint; expected SHA256:... or 64 hex digits"
    )))
}

/// Parse a public key from its transmitted form.
pub fn parse_public(raw: &str) -> Result<VerifyingKey, Error> {
    let bytes = STANDARD_NO_PAD
        .decode(raw.trim())
        .map_err(|e| Error::usage(format!("'{raw}' is not a public key: {e}")))?;
    let array: [u8; 32] = bytes
        .try_into()
        .map_err(|_| Error::usage(format!("'{raw}' is not a public key: wrong length")))?;
    VerifyingKey::from_bytes(&array)
        .map_err(|e| Error::usage(format!("'{raw}' is not a public key: {e}")))
}

/// The key a record claims, or `None` when it carries no signature this build can read.
pub fn claim(value: &Value) -> Option<Claim> {
    let sig = value.get("meta")?.get("sig")?.as_object()?;
    if sig.get("alg")?.as_str()? != ALG {
        return None;
    }
    let key = parse_public(sig.get("key")?.as_str()?).ok()?;
    Some(Claim {
        fingerprint: fingerprint(&key),
        key,
    })
}

/// The signature bytes a record carries, if it carries one.
fn signature_bytes(value: &Value) -> Option<Vec<u8>> {
    let sig = value.get("meta")?.get("sig")?.as_object()?;
    STANDARD_NO_PAD.decode(sig.get("sig")?.as_str()?).ok()
}

/// What a signature covers: the canonical sender, the canonical address, the type, and the digest
/// of the body as stored.
///
/// Every field is length-prefixed, so no two distinct records encode to one byte string and no
/// field can be shifted into its neighbour.
pub fn signed_bytes(value: &Value) -> Result<Vec<u8>, Error> {
    let text = |name: &str| -> Result<String, Error> {
        value
            .get(name)
            .and_then(Value::as_str)
            .map(str::to_string)
            .ok_or_else(|| Error::usage(format!("a signed record must carry '{name}'")))
    };
    let from = address::canonicalize(&text("from")?)?;
    let to = address::canonicalize(&text("to")?)?;
    let kind = text("type")?;

    let mut out = Vec::new();
    out.extend_from_slice(DOMAIN);
    for field in [from.as_bytes(), to.as_bytes(), kind.as_bytes()] {
        out.extend_from_slice(&(field.len() as u32).to_be_bytes());
        out.extend_from_slice(field);
    }
    let digest = Sha256::digest(body_bytes(value));
    out.extend_from_slice(&digest);
    Ok(out)
}

/// The body as it is stored: the message bytes, or the event as JSON, or nothing.
///
/// The seal step runs before signing, so a sealed body is signed as the bytes that are stored.
fn body_bytes(value: &Value) -> Vec<u8> {
    if let Some(message) = value.get("message").and_then(Value::as_str) {
        return message.as_bytes().to_vec();
    }
    if let Some(event) = value.get("event") {
        return serde_json::to_vec(event).unwrap_or_default();
    }
    Vec::new()
}

/// Sign a record in place, or say why it cannot be signed.
pub fn sign_value(value: &mut Value, signing: &SigningKey) -> Result<(), Error> {
    let bytes = signed_bytes(value)?;
    let signature = signing.sign(&bytes);
    let sig = serde_json::json!({
        "alg": ALG,
        "key": public(&signing.verifying_key()),
        "sig": STANDARD_NO_PAD.encode(signature.to_bytes()),
    });
    match value.get_mut("meta").and_then(Value::as_object_mut) {
        Some(meta) => {
            meta.insert("sig".to_string(), sig);
        }
        None => {
            value["meta"] = serde_json::json!({ "sig": sig });
        }
    }
    Ok(())
}

/// Whether a record's signature verifies against the key it claims.
pub fn verify_claim(value: &Value, claim: &Claim) -> Result<(), String> {
    let bytes = signed_bytes(value).map_err(|e| e.to_string())?;
    let raw = signature_bytes(value).ok_or_else(|| "the signature is not readable".to_string())?;
    let array: [u8; 64] = raw
        .try_into()
        .map_err(|_| "the signature is not 64 bytes".to_string())?;
    claim
        .key
        .verify(&bytes, &Signature::from_bytes(&array))
        .map_err(|_| "the signature does not cover this record".to_string())
}

/// Write a private file nobody else may read.
#[cfg(unix)]
fn write_private(path: &Path, bytes: &[u8]) -> Result<(), Error> {
    use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};

    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(path)
        .map_err(|e| Error::runtime(format!("cannot write {}: {e}", path.display())))?;
    file.write_all(bytes)
        .map_err(|e| Error::runtime(format!("cannot write {}: {e}", path.display())))?;
    // mode() only applies on creation; an existing file keeps its old bits without this.
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
        .map_err(|e| Error::runtime(format!("cannot restrict {}: {e}", path.display())))?;
    Ok(())
}

#[cfg(not(unix))]
fn write_private(path: &Path, bytes: &[u8]) -> Result<(), Error> {
    std::fs::write(path, bytes)
        .map_err(|e| Error::runtime(format!("cannot write {}: {e}", path.display())))
}

#[cfg(test)]
mod tests {
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
        let raw =
            std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
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
    fn a_principal_name_is_not_a_path() {
        assert!(key_path("alice").is_ok());
        assert!(key_path("session/alice").is_err());
        assert!(key_path("").is_err());
        assert!(key_path("..").is_err());
    }
}
