//! Presence records, as the roster reads them.
//!
//! Presence is public: any process that can publish may appear, which is the point. The record's
//! shape is described in `PROTOCOL.md`; this module only parses it, tolerantly.

use serde_json::Value;

/// The address every participant publishes presence to, and the roster reads.
pub const PRESENCE_ADDRESS: &str = "group/presence";

/// How many of the newest presence records the roster reads.
///
/// Presence is a fixed-size window in practice: one record per participant per renewal, so a few
/// hundred of them cover far more live participants than a bus ever has. Reading the tail rather
/// than the stream keeps `sessions` cheap on a bus that has been running for months.
pub const SCAN_RECORDS: i64 = 512;

/// The lifetime assumed when a record does not declare one.
pub const DEFAULT_TTL_S: i64 = 30;

/// One live participant, ready to print.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Roster {
    pub address: String,
    pub kind: String,
    pub state: String,
    pub alias: Option<String>,
    pub expires_at: i64,
}

/// One parsed presence record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PresenceRecord {
    /// The address the publisher answers at: the record's `from`.
    pub address: String,
    /// Stable identity, so two participants sharing an alias stay distinct.
    pub participant: String,
    pub alias: Option<String>,
    pub state: String,
    /// Declared lifetime of this record, in seconds.
    pub ttl: i64,
}

/// Parse a stored payload as a presence record. Anything unrecognized is not presence.
///
/// `participant` falls back to the address, so a client that publishes nothing but `from`, `state`
/// and a lifetime still appears — which is the whole reason presence is a record.
pub fn parse_presence(record: &Value) -> Option<PresenceRecord> {
    if record.get("type")?.as_str()? != "event" {
        return None;
    }
    let event = record.get("event")?.as_object()?;
    if event.get("kind")?.as_str()? != "presence" {
        return None;
    }

    let address = record.get("from")?.as_str()?.trim();
    if address.is_empty() {
        return None;
    }
    let state = event.get("state")?.as_str()?.trim();
    if state.is_empty() {
        return None;
    }

    let ttl = event
        .get("ttl")
        .and_then(Value::as_i64)
        .filter(|ttl| *ttl > 0)
        .unwrap_or(DEFAULT_TTL_S);
    let participant = event
        .get("participant")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(address)
        .to_string();
    let alias = event
        .get("alias")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string);

    Some(PresenceRecord {
        address: address.to_string(),
        participant,
        alias,
        state: state.to_string(),
        ttl,
    })
}

/// Whether a record retracts presence instead of describing it.
pub fn is_retraction(state: &str) -> bool {
    state.eq_ignore_ascii_case("gone")
}

/// The scheme of an address, which is what a participant is: `session`, `cli`, `group`, `cwd`.
pub fn kind_of(address: &str) -> String {
    address
        .split_once('/')
        .map(|(scheme, _)| scheme)
        .unwrap_or("group")
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parses_a_complete_record() {
        let payload = json!({
            "v": 1, "type": "event", "from": "session/api-worker", "to": PRESENCE_ADDRESS,
            "event": {"kind": "presence", "state": "busy", "ttl": 30,
                      "participant": "01a127d9", "alias": "api-worker"}
        });
        let record = parse_presence(&payload).unwrap();
        assert_eq!(record.address, "session/api-worker");
        assert_eq!(record.participant, "01a127d9");
        assert_eq!(record.alias.as_deref(), Some("api-worker"));
        assert_eq!(record.state, "busy");
        assert_eq!(record.ttl, 30);
    }

    #[test]
    fn a_client_that_publishes_barely_anything_still_appears() {
        let payload = json!({
            "v": 1, "type": "event", "from": "cli/ci@host", "to": PRESENCE_ADDRESS,
            "event": {"kind": "presence", "state": "idle"}
        });
        let record = parse_presence(&payload).unwrap();
        assert_eq!(
            record.participant, "cli/ci@host",
            "the address is the fallback identity"
        );
        assert_eq!(record.alias, None);
        assert_eq!(record.ttl, DEFAULT_TTL_S);
        assert_eq!(kind_of(&record.address), "cli");
    }

    #[test]
    fn a_record_that_is_not_presence_is_ignored() {
        for payload in [
            json!({"v": 1, "type": "note", "from": "a", "to": "b", "message": "hi"}),
            json!({"v": 1, "type": "event", "from": "a", "to": "b", "event": {"kind": "ack", "phase": "started"}}),
            json!({"v": 1, "type": "event", "from": "a", "to": "b", "event": {"kind": "presence"}}),
            json!({"v": 1, "type": "event", "to": "b", "event": {"kind": "presence", "state": "idle"}}),
            json!(["not", "an", "object"]),
        ] {
            assert_eq!(parse_presence(&payload), None, "{payload}");
        }
    }

    #[test]
    fn retraction_is_recognized() {
        assert!(is_retraction("gone"));
        assert!(!is_retraction("idle"));
    }
}
