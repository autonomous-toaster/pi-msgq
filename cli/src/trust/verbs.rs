//! The trust verbs: what a person, and only a person, records about a peer.
//!
//! Every function here changes what a reader believes, so none of them is reachable from a command
//! that does anything else. A record arriving on a bus can never call one, and no bus is asked for
//! permission: trust is recorded against a bus, by the person at the keyboard.

use serde_json::json;

use crate::config::Target;
use crate::error::Error;
use crate::keys;
use crate::store::{Bus, Store};
use crate::trust::{self, Anchor, Layer, Located, Retired};

/// What recording an anchor changed.
pub struct Recorded {
    pub anchor: Anchor,
    pub path: std::path::PathBuf,
    /// Whether the principal was already trusted at this fingerprint.
    pub unchanged: bool,
}

/// What rotating an anchor broke.
pub struct Rotated {
    pub previous: String,
    pub anchor: Anchor,
    pub path: std::path::PathBuf,
    /// Retained records whose signature names the key that just stopped being current.
    pub refused_records: i64,
}

/// Record that a principal is trusted at a fingerprint.
///
/// Recording over a different fingerprint is refused and named as a rotation, because one principal
/// holding two current keys is the thing the keyring exists to prevent.
pub fn add(target: &Target, principal: &str, fingerprint: &str) -> Result<Recorded, Error> {
    let fingerprint = keys::normalize_fingerprint(fingerprint)?;
    let principal = principal.trim().to_string();
    if principal.is_empty() {
        return Err(Error::usage("a trust anchor needs a principal"));
    }

    let mut anchors = trust::load_durable(target)?;
    claim_fingerprint(&anchors, &principal, &fingerprint)?;

    let mut unchanged = false;
    match anchors.iter_mut().find(|a| a.principal == principal) {
        Some(existing) if existing.current == fingerprint => unchanged = true,
        Some(existing) => {
            return Err(Error::usage(format!(
                "{principal} is already trusted at {}; rotate it to {} instead",
                existing.current, fingerprint
            )));
        }
        None => anchors.push(Anchor::new(principal.clone(), fingerprint.clone())),
    }

    let path = trust::save_durable(target, &anchors)?;
    Ok(Recorded {
        anchor: Anchor::new(principal, fingerprint),
        path,
        unchanged,
    })
}

/// Stop trusting a principal, and forget what it used to be trusted at.
pub fn revoke(target: &Target, principal: &str) -> Result<std::path::PathBuf, Error> {
    let mut anchors = trust::load_durable(target)?;
    let before = anchors.len();
    anchors.retain(|a| a.principal != principal.trim());
    if anchors.len() == before {
        return Err(Error::usage(format!(
            "{} is not trusted on this bus",
            principal.trim()
        )));
    }
    trust::save_durable(target, &anchors)
}

/// Replace a principal's key, retiring the old one, and report what that costs.
///
/// Rotation is the one trust act that destroys value: every retained record signed by the old key
/// stops being trusted. The count is what stops that being a surprise.
pub fn rotate(
    target: &Target,
    principal: &str,
    fingerprint: &str,
    today: &str,
) -> Result<Rotated, Error> {
    let fingerprint = keys::normalize_fingerprint(fingerprint)?;
    let principal = principal.trim().to_string();
    let mut anchors = trust::load_durable(target)?;
    claim_fingerprint(&anchors, &principal, &fingerprint)?;

    let previous = {
        let anchor = anchors
            .iter_mut()
            .find(|a| a.principal == principal)
            .ok_or_else(|| {
                Error::usage(format!(
                    "{principal} is not trusted on this bus; record an anchor before rotating one"
                ))
            })?;
        if anchor.current == fingerprint {
            return Err(Error::usage(format!(
                "{principal} is already trusted at {fingerprint}"
            )));
        }
        let previous = anchor.current.clone();
        anchor.retired.push(Retired {
            fingerprint: previous.clone(),
            retired_at: today.to_string(),
        });
        anchor.current = fingerprint.clone();
        previous
    };

    let refused_records = count_signed_by(target, &previous)?;
    let path = trust::save_durable(target, &anchors)?;
    let mut anchor = Anchor::new(principal, fingerprint);
    anchor.retired.push(Retired {
        fingerprint: previous.clone(),
        retired_at: today.to_string(),
    });
    Ok(Rotated {
        previous,
        anchor,
        path,
        refused_records,
    })
}

/// Refuse a fingerprint that already belongs to another principal, and name the one holding it.
fn claim_fingerprint(anchors: &[Anchor], principal: &str, fingerprint: &str) -> Result<(), Error> {
    if let Some(holder) = anchors
        .iter()
        .find(|a| a.principal != principal && a.current == fingerprint)
    {
        return Err(Error::usage(format!(
            "{fingerprint} is already trusted for {}; one key belongs to one principal",
            holder.principal
        )));
    }
    Ok(())
}

/// How many retained records are signed by one fingerprint.
///
/// Bounded by the retention window, which the bus declares, so this is a cost the operator can see
/// coming rather than an unbounded scan of history.
fn count_signed_by(target: &Target, fingerprint: &str) -> Result<i64, Error> {
    let (store, _) = Store::open(target)?;
    let mut total = 0;
    for topic in store.list_topics_names()? {
        let mut offset = 0;
        loop {
            let batch = store.read_since(&topic, offset, 500)?;
            if batch.is_empty() {
                break;
            }
            for event in &batch {
                offset = event.offset;
                let value: serde_json::Value =
                    serde_json::from_slice(&event.payload).unwrap_or(serde_json::Value::Null);
                if keys::claim(&value).is_some_and(|c| c.fingerprint == fingerprint) {
                    total += 1;
                }
            }
        }
    }
    Ok(total)
}

/// Every anchor in force for this bus, and where each came from.
pub struct Listing {
    pub bus: String,
    pub path: std::path::PathBuf,
    pub in_force: Vec<Located>,
    pub conflicts: Vec<trust::Conflict>,
    pub durable: Vec<Anchor>,
}

pub fn list(target: &Target) -> Result<Listing, Error> {
    Ok(Listing {
        bus: trust::bus_id(target)?,
        path: trust::keyring_path(target)?,
        in_force: trust::anchors_in_force(target)?,
        conflicts: trust::conflicts(target)?,
        durable: trust::load_durable(target)?,
    })
}

/// The listing as the reader sees it: one line per principal, naming the layer that decided.
pub fn describe(listing: &Listing, insecure: bool) -> Vec<String> {
    let mut lines = vec![
        format!("bus:        {}", listing.bus),
        format!("keyring:    {}", listing.path.display()),
        format!(
            "unsigned:   {}",
            if insecure {
                "read (MSGQ_INSECURE is set)"
            } else {
                "refused"
            }
        ),
    ];
    if listing.in_force.is_empty() {
        lines.push("anchors:    none; every sender is unverified".to_string());
    } else {
        lines.push("anchors:".to_string());
        for located in &listing.in_force {
            let anchor = &located.anchor;
            let retired: Vec<String> = anchor
                .retired
                .iter()
                .map(|r| format!("{} retired {}", trust::short(&r.fingerprint), r.retired_at))
                .collect();
            lines.push(format!(
                "  {:<20} {:<16} {}{}",
                anchor.principal,
                trust::short(&anchor.current),
                located.layer.name(),
                if retired.is_empty() {
                    String::new()
                } else {
                    format!(" (also {})", retired.join(", "))
                }
            ));
        }
    }
    for conflict in &listing.conflicts {
        let claims: Vec<String> = conflict
            .claims
            .iter()
            .map(|(layer, fingerprint)| {
                format!("{} says {}", layer.name(), trust::short(fingerprint))
            })
            .collect();
        lines.push(format!(
            "disagreement: {} — {}",
            conflict.principal,
            claims.join(", ")
        ));
    }
    lines
}

/// The listing as JSON, for a caller that wants to render it itself.
pub fn describe_json(listing: &Listing, insecure: bool) -> serde_json::Value {
    json!({
        "bus": listing.bus,
        "keyring": listing.path.display().to_string(),
        "unsigned": if insecure { "read" } else { "refused" },
        "anchors": listing.in_force.iter().map(|located| {
            let mut value = located.anchor.to_json();
            value["layer"] = json!(located.layer.name());
            value
        }).collect::<Vec<_>>(),
        "durable": listing.durable.iter().map(|a| a.to_json()).collect::<Vec<_>>(),
        "conflicts": listing.conflicts.iter().map(|c| json!({
            "principal": c.principal,
            "claims": c.claims.iter().map(|(layer, fingerprint)| json!({
                "layer": layer.name(),
                "fingerprint": fingerprint,
            })).collect::<Vec<_>>(),
        })).collect::<Vec<_>>(),
    })
}

/// The layer a durable anchor lives in, so a caller can label it without guessing.
pub const DURABLE: Layer = Layer::Durable;
