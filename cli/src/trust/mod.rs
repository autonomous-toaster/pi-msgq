//! Trust anchors: what a reader believes about a peer, where that belief lives, and the verdict it
//! reaches about one record.
//!
//! Nothing here asks the bus anything. A verdict is the reader's own opinion, reached by checking a
//! signature against a fingerprint the reader was given out of band.

use std::path::PathBuf;

use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use crate::config::{self, Target};
use crate::error::Error;
use crate::keys;

/// Anchors for one invocation.
pub const TRUST_ENV: &str = "MSGQ_TRUST";

/// Anchors the pi session holds in memory, handed down to the client it spawns.
pub const SESSION_ENV: &str = "MSGQ_TRUST_SESSION";

/// Set to permit reading records that carry no usable signature.
pub const INSECURE_ENV: &str = "MSGQ_INSECURE";

/// A key that used to be a principal's, and when it stopped being one.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Retired {
    pub fingerprint: String,
    pub retired_at: String,
}

/// What a reader believes about one principal.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Anchor {
    pub principal: String,
    pub current: String,
    pub retired: Vec<Retired>,
}

impl Anchor {
    pub fn new(principal: impl Into<String>, fingerprint: impl Into<String>) -> Self {
        Anchor {
            principal: principal.into(),
            current: fingerprint.into(),
            retired: Vec::new(),
        }
    }

    pub fn to_json(&self) -> Value {
        json!({
            "principal": self.principal,
            "current": self.current,
            "retired": self.retired.iter().map(|r| json!({
                "fingerprint": r.fingerprint,
                "retired_at": r.retired_at,
            })).collect::<Vec<_>>(),
        })
    }

    fn from_json(value: &Value) -> Result<Self, Error> {
        let bad = |what: &str| Error::runtime(format!("a stored anchor has no {what}"));
        let principal = value
            .get("principal")
            .and_then(Value::as_str)
            .ok_or_else(|| bad("principal"))?;
        let current = value
            .get("current")
            .and_then(Value::as_str)
            .ok_or_else(|| bad("current fingerprint"))?;
        let retired = value
            .get("retired")
            .and_then(Value::as_array)
            .map(|rows| {
                rows.iter()
                    .filter_map(|row| {
                        Some(Retired {
                            fingerprint: row.get("fingerprint")?.as_str()?.to_string(),
                            retired_at: row
                                .get("retired_at")
                                .and_then(Value::as_str)
                                .unwrap_or_default()
                                .to_string(),
                        })
                    })
                    .collect()
            })
            .unwrap_or_default();
        Ok(Anchor {
            principal: principal.to_string(),
            current: keys::normalize_fingerprint(current)?,
            retired,
        })
    }
}

/// Which layer an anchor came from. The order here is the precedence order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Layer {
    Environment,
    Session,
    Durable,
}

impl Layer {
    pub fn name(self) -> &'static str {
        match self {
            Layer::Environment => "environment",
            Layer::Session => "session",
            Layer::Durable => "durable",
        }
    }
}

/// An anchor and where it was found.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Located {
    pub anchor: Anchor,
    pub layer: Layer,
}

/// What a reader concludes about one record.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Verdict {
    Trusted {
        principal: String,
        fingerprint: String,
    },
    Unverified {
        fingerprint: Option<String>,
        reason: String,
    },
    Refused {
        fingerprint: String,
        reason: String,
    },
}

impl Verdict {
    pub fn state(&self) -> &'static str {
        match self {
            Verdict::Trusted { .. } => "trusted",
            Verdict::Unverified { .. } => "unverified",
            Verdict::Refused { .. } => "refused",
        }
    }

    pub fn is_unsigned(&self) -> bool {
        matches!(
            self,
            Verdict::Unverified {
                fingerprint: None,
                ..
            }
        )
    }

    pub fn to_json(&self) -> Value {
        match self {
            Verdict::Trusted {
                principal,
                fingerprint,
            } => json!({ "state": "trusted", "principal": principal, "fingerprint": fingerprint }),
            Verdict::Unverified {
                fingerprint,
                reason,
            } => json!({
                "state": "unverified",
                "fingerprint": fingerprint,
                "reason": reason,
            }),
            Verdict::Refused {
                fingerprint,
                reason,
            } => json!({
                "state": "refused",
                "fingerprint": fingerprint,
                "reason": reason,
            }),
        }
    }

    /// The one-line form, for a text column and for `msgq verify`.
    pub fn line(&self) -> String {
        match self {
            Verdict::Trusted {
                principal,
                fingerprint,
            } => format!("trusted    {principal} {}", short(fingerprint)),
            Verdict::Unverified {
                fingerprint: Some(fingerprint),
                ..
            } => format!("unverified {}", short(fingerprint)),
            Verdict::Unverified {
                fingerprint: None, ..
            } => "unverified unsigned".to_string(),
            Verdict::Refused {
                fingerprint,
                reason,
            } => format!("refused    {} ({reason})", short(fingerprint)),
        }
    }
}

/// A fingerprint, shortened for a column. Comparison happens on the whole value; this is for eyes.
pub fn short(fingerprint: &str) -> String {
    let body = fingerprint.strip_prefix("SHA256:").unwrap_or(fingerprint);
    format!("SHA256:{}", &body[..body.len().min(8)])
}

/// Whether this invocation is permitted to read records with no usable signature.
pub fn insecure() -> bool {
    match std::env::var(INSECURE_ENV) {
        Ok(raw) => matches!(raw.trim(), "1" | "true" | "yes"),
        Err(_) => false,
    }
}

/// A stable, readable name for one bus.
///
/// Trust is per bus, so the name has to be the same across invocations and different between buses.
/// It follows `pg/naming.rs`: a readable slug for a human, a digest for identity.
pub fn bus_id(target: &Target) -> Result<String, Error> {
    let name = match target {
        Target::Local(path) => format!("local:{}", absolute(path)?),
        Target::Shared(dsn) if !dsn.trim().is_empty() => format!("shared:{}", redact(dsn)),
        Target::Shared(_) => {
            let settings: Vec<String> = config::pg_settings()
                .into_iter()
                .filter(|(variable, _)| !matches!(*variable, "MSGQ_PGPASSWORD" | "MSGQ_PGSSLMODE"))
                .map(|(_, value)| value)
                .collect();
            format!("shared:{}", settings.join("/"))
        }
    };
    let digest = Sha256::digest(name.as_bytes());
    let suffix: String = digest.iter().take(8).map(|b| format!("{b:02x}")).collect();
    let slug: String = name
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect();
    let slug = slug.trim_matches('_');
    let slug = &slug[..slug.len().min(40)];
    Ok(format!("{slug}_{suffix}").to_lowercase())
}

fn absolute(path: &std::path::Path) -> Result<String, Error> {
    if let Ok(resolved) = std::fs::canonicalize(path) {
        return Ok(resolved.display().to_string());
    }
    // The bus may not exist yet — a keyring is written before the first publish — so the path has to
    // be settled without asking the filesystem, and two spellings of it have to land on one name.
    let cwd = std::env::current_dir()
        .map_err(|e| Error::runtime(format!("cannot resolve the bus path: {e}")))?;
    Ok(lexical(&cwd.join(path)).display().to_string())
}

/// Resolve `.` and `..` without touching the filesystem.
fn lexical(path: &std::path::Path) -> std::path::PathBuf {
    use std::path::Component;
    let mut out = std::path::PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                if !out.pop() {
                    out.push(component);
                }
            }
            other => out.push(other),
        }
    }
    out
}

/// Drop any password from a connection string before it becomes part of a directory name.
fn redact(dsn: &str) -> String {
    let mut kept: Vec<String> = Vec::new();
    for part in dsn.split_whitespace() {
        if part.to_lowercase().starts_with("password=") {
            continue;
        }
        kept.push(part.to_string());
    }
    let joined = kept.join(" ");
    // The URI form carries the password in the userinfo: postgres://user:secret@host/db.
    if let Some((scheme, rest)) = joined.split_once("://")
        && let Some((userinfo, host)) = rest.split_once('@')
    {
        let user = userinfo.split(':').next().unwrap_or_default();
        return format!("{scheme}://{user}@{host}");
    }
    joined
}

/// The file holding one bus's anchors.
pub fn keyring_path(target: &Target) -> Result<PathBuf, Error> {
    let dir = config::state_dir()
        .ok_or_else(|| Error::runtime("cannot find a home directory for the trust keyring"))?;
    Ok(dir.join("trust").join(bus_id(target)?).join("anchors.json"))
}

/// Read one bus's durable anchors.
///
/// A damaged keyring is an error rather than an empty keyring: "nothing is trusted" and "the file
/// could not be read" are different facts, and only one of them is safe to guess.
pub fn load_durable(target: &Target) -> Result<Vec<Anchor>, Error> {
    let path = keyring_path(target)?;
    let Ok(raw) = std::fs::read_to_string(&path) else {
        return Ok(Vec::new());
    };
    let doc: Value = serde_json::from_str(&raw)
        .map_err(|e| Error::runtime(format!("{} is not readable: {e}", path.display())))?;
    let rows = doc
        .get("anchors")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    rows.iter().map(Anchor::from_json).collect()
}

/// Write one bus's durable anchors, atomically: a half-written keyring would change what a reader
/// believes without anybody asking for it.
pub fn save_durable(target: &Target, anchors: &[Anchor]) -> Result<PathBuf, Error> {
    let path = keyring_path(target)?;
    let dir = path
        .parent()
        .ok_or_else(|| Error::runtime("the keyring has no directory"))?;
    std::fs::create_dir_all(dir)
        .map_err(|e| Error::runtime(format!("cannot create {}: {e}", dir.display())))?;

    let mut sorted: Vec<&Anchor> = anchors.iter().collect();
    sorted.sort_by(|a, b| a.principal.cmp(&b.principal));
    let doc = json!({
        "v": 1,
        "bus": bus_id(target)?,
        "anchors": sorted.iter().map(|a| a.to_json()).collect::<Vec<_>>(),
    });
    let body = serde_json::to_string_pretty(&doc)?;

    let temp = path.with_extension("json.tmp");
    std::fs::write(&temp, body)
        .map_err(|e| Error::runtime(format!("cannot write {}: {e}", temp.display())))?;
    std::fs::rename(&temp, &path)
        .map_err(|e| Error::runtime(format!("cannot replace {}: {e}", path.display())))?;
    Ok(path)
}

/// Parse a layer written in the environment: `principal:fingerprint`, one or more, comma- or
/// whitespace-separated.
///
/// A malformed entry and a contradictory pair are both fatal. Skipping either would turn a typo into
/// "trusted nobody", or a conflict into a coin flip.
pub fn parse_layer(variable: &str, raw: &str, layer: Layer) -> Result<Vec<Located>, Error> {
    let mut out: Vec<Located> = Vec::new();
    for entry in raw
        .split([',', ' ', '\t', '\n'])
        .filter(|e| !e.trim().is_empty())
    {
        let entry = entry.trim();
        let (principal, fingerprint) = entry.split_once(':').ok_or_else(|| {
            Error::usage(format!(
                "{variable}: '{entry}' is not principal:fingerprint"
            ))
        })?;
        let principal = principal.trim();
        if principal.is_empty() {
            return Err(Error::usage(format!(
                "{variable}: '{entry}' names no principal"
            )));
        }
        let fingerprint = keys::normalize_fingerprint(fingerprint)?;
        match out.iter_mut().find(|l| l.anchor.principal == principal) {
            Some(existing) if existing.anchor.current == fingerprint => {}
            Some(existing) => {
                return Err(Error::usage(format!(
                    "{variable}: {principal} is given two fingerprints, {} and {}",
                    existing.anchor.current, fingerprint
                )));
            }
            None => out.push(Located {
                anchor: Anchor::new(principal, fingerprint),
                layer,
            }),
        }
    }
    Ok(out)
}

/// The anchors a layer in the environment names, if it is set at all.
fn env_layer(variable: &str, layer: Layer) -> Result<Vec<Located>, Error> {
    match std::env::var(variable) {
        Ok(raw) if !raw.trim().is_empty() => parse_layer(variable, &raw, layer),
        _ => Ok(Vec::new()),
    }
}

/// Every anchor in force for one bus, most transient layer first, with one entry per principal.
pub fn anchors_in_force(target: &Target) -> Result<Vec<Located>, Error> {
    let mut out: Vec<Located> = Vec::new();
    let mut layers = env_layer(TRUST_ENV, Layer::Environment)?;
    layers.extend(env_layer(SESSION_ENV, Layer::Session)?);
    layers.extend(load_durable(target)?.into_iter().map(|anchor| Located {
        anchor,
        layer: Layer::Durable,
    }));
    for located in layers {
        if !out
            .iter()
            .any(|l| l.anchor.principal == located.anchor.principal)
        {
            out.push(located);
        }
    }
    Ok(out)
}

/// A principal whose layers disagree, and what each of them says.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Conflict {
    pub principal: String,
    pub claims: Vec<(Layer, String)>,
}

/// Every principal whose layers name different fingerprints, most transient layer first.
pub fn conflicts(target: &Target) -> Result<Vec<Conflict>, Error> {
    let mut seen: Vec<(String, Vec<(Layer, String)>)> = Vec::new();
    let mut layers = env_layer(TRUST_ENV, Layer::Environment)?;
    layers.extend(env_layer(SESSION_ENV, Layer::Session)?);
    layers.extend(load_durable(target)?.into_iter().map(|anchor| Located {
        anchor,
        layer: Layer::Durable,
    }));
    for located in layers {
        let entry = match seen
            .iter_mut()
            .find(|(p, _)| *p == located.anchor.principal)
        {
            Some(entry) => entry,
            None => {
                seen.push((located.anchor.principal.clone(), Vec::new()));
                seen.last_mut().expect("just pushed")
            }
        };
        entry.1.push((located.layer, located.anchor.current));
    }
    Ok(seen
        .into_iter()
        .filter(|(_, claims)| claims.iter().any(|(_, fp)| *fp != claims[0].1))
        .map(|(principal, claims)| Conflict { principal, claims })
        .collect())
}

/// A reader: the anchors in force, held once so a batch costs one keyring read.
pub struct Reader {
    anchors: Vec<Located>,
}

impl Reader {
    pub fn new(target: &Target) -> Result<Self, Error> {
        Ok(Reader {
            anchors: anchors_in_force(target)?,
        })
    }

    pub fn anchors(&self) -> &[Located] {
        &self.anchors
    }

    /// The principal a fingerprint belongs to, current or retired.
    fn principal_of(&self, fingerprint: &str) -> Option<String> {
        self.anchors
            .iter()
            .find(|l| {
                l.anchor.current == fingerprint
                    || l.anchor
                        .retired
                        .iter()
                        .any(|r| r.fingerprint == fingerprint)
            })
            .map(|l| l.anchor.principal.clone())
    }

    /// Reach a verdict about one record.
    ///
    /// Order matters: a broken signature is refused whatever the anchors say, because a record whose
    /// bytes changed is not the record that was signed.
    pub fn verdict(&self, value: &Value) -> Verdict {
        let Some(claim) = keys::claim(value) else {
            return Verdict::Unverified {
                fingerprint: None,
                reason: "no usable signature".to_string(),
            };
        };
        if let Err(reason) = keys::verify_claim(value, &claim) {
            // Name the principal when the claimed key is one this reader knows, so a refusal says who
            // is being impersonated rather than only which bytes are wrong.
            let reason = match self.principal_of(&claim.fingerprint) {
                Some(principal) => format!("{reason} (claimed by {principal})"),
                None => reason,
            };
            return Verdict::Refused {
                fingerprint: claim.fingerprint,
                reason,
            };
        }
        if let Some(located) = self
            .anchors
            .iter()
            .find(|l| l.anchor.current == claim.fingerprint)
        {
            return Verdict::Trusted {
                principal: located.anchor.principal.clone(),
                fingerprint: claim.fingerprint,
            };
        }
        if let Some(located) = self.anchors.iter().find(|l| {
            l.anchor
                .retired
                .iter()
                .any(|r| r.fingerprint == claim.fingerprint)
        }) {
            let when = located
                .anchor
                .retired
                .iter()
                .find(|r| r.fingerprint == claim.fingerprint)
                .map(|r| r.retired_at.clone())
                .unwrap_or_default();
            return Verdict::Refused {
                fingerprint: claim.fingerprint,
                reason: format!("signed by a key retired on {when}"),
            };
        }
        Verdict::Unverified {
            fingerprint: Some(claim.fingerprint),
            reason: "no anchor for this key".to_string(),
        }
    }
}

pub mod verbs;

#[cfg(test)]
mod tests;
