//! The message record contract.
//!
//! A record is a pi-native JSON object. `type` maps onto pi's own vocabulary
//! so a future pi extension dispatches without translation.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::error::Error;

/// Record protocol version. A record's `v` must equal this.
pub const PROTOCOL_VERSION: u32 = 1;

/// Record types this version produces.
pub const KNOWN_TYPES: [&str; 6] = ["prompt", "steer", "follow_up", "note", "command", "event"];

/// A msgq message record.
///
/// Unknown top-level fields are preserved in `extra` so a record supplied
/// through passthrough is stored without losing fields.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Record {
    pub v: u32,
    #[serde(rename = "type")]
    pub kind: String,
    pub from: String,
    pub to: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub images: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub event: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reply_to: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub meta: Option<Value>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

impl Record {
    /// Build a message-bearing record in shorthand mode.
    pub fn message(kind: &str, from: &str, to: &str, message: impl Into<String>) -> Self {
        Record {
            v: PROTOCOL_VERSION,
            kind: kind.to_string(),
            from: from.to_string(),
            to: to.to_string(),
            message: Some(message.into()),
            images: None,
            event: None,
            reply_to: None,
            meta: None,
            extra: Map::new(),
        }
    }
}

/// Parse a JSON value into a record and validate it.
pub fn parse_and_validate(value: &Value) -> Result<Record, Error> {
    let rec: Record = serde_json::from_value(value.clone())
        .map_err(|e| Error::usage(format!("invalid record: {e}")))?;
    validate(&rec)?;
    Ok(rec)
}

/// Validate a record against the contract.
pub fn validate(rec: &Record) -> Result<(), Error> {
    if rec.v != PROTOCOL_VERSION {
        return Err(Error::usage(format!(
            "unsupported record version {}; expected {}",
            rec.v, PROTOCOL_VERSION
        )));
    }
    if !KNOWN_TYPES.contains(&rec.kind.as_str()) {
        return Err(Error::usage(format!(
            "unknown record type '{}'; expected one of {}",
            rec.kind,
            KNOWN_TYPES.join(", ")
        )));
    }
    if rec.kind == "event" {
        if rec.event.is_none() {
            return Err(Error::usage(
                "record of type 'event' requires an 'event' field",
            ));
        }
    } else if rec.message.is_none() {
        return Err(Error::usage(format!(
            "record of type '{}' requires a 'message' field",
            rec.kind
        )));
    }
    Ok(())
}

/// Serialize a record to a JSON value.
pub fn to_value(rec: &Record) -> Result<Value, Error> {
    Ok(serde_json::to_value(rec)?)
}

/// The default `from` for a non-pi producer.
pub fn default_sender() -> String {
    let user = std::env::var("USER")
        .or_else(|_| std::env::var("USERNAME"))
        .unwrap_or_else(|_| "unknown".to_string());
    format!("cli/{user}@{}", hostname())
}

fn hostname() -> String {
    if let Ok(h) = std::env::var("HOSTNAME")
        && !h.trim().is_empty()
    {
        return h.trim().to_string();
    }
    std::process::Command::new("hostname")
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "localhost".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn command_is_a_message_type() {
        // A command carries a command line, and the bus does not know which commands exist.
        let rec = Record::message(
            "command",
            "cli/a@b",
            "session/x",
            "/harsh-review the parser",
        );
        assert!(validate(&rec).is_ok());
        let value = to_value(&rec).unwrap();
        assert_eq!(value["type"], json!("command"));
        assert_eq!(value["message"], json!("/harsh-review the parser"));
        assert_eq!(parse_and_validate(&value).unwrap(), rec);
    }

    #[test]
    fn round_trip_message_record() {
        let rec = Record::message("prompt", "cli/a@b", "group/x", "hello");
        let value = to_value(&rec).unwrap();
        assert_eq!(value["v"], json!(1));
        assert_eq!(value["type"], json!("prompt"));
        assert_eq!(value["message"], json!("hello"));
        let back = parse_and_validate(&value).unwrap();
        assert_eq!(back, rec);
    }

    #[test]
    fn unknown_fields_are_preserved() {
        let value = json!({
            "v": 1, "type": "note", "from": "a", "to": "b",
            "message": "hi", "custom": {"k": [1, 2]}
        });
        let rec = parse_and_validate(&value).unwrap();
        assert!(rec.extra.contains_key("custom"));
    }

    #[test]
    fn missing_required_field_is_rejected() {
        let value = json!({"v": 1, "type": "note", "from": "a", "message": "hi"});
        assert!(parse_and_validate(&value).is_err());
    }

    #[test]
    fn bad_version_is_rejected() {
        let value = json!({"v": 2, "type": "note", "from": "a", "to": "b", "message": "hi"});
        assert!(parse_and_validate(&value).is_err());
    }

    #[test]
    fn event_requires_event_field() {
        let value = json!({"v": 1, "type": "event", "from": "a", "to": "b"});
        assert!(parse_and_validate(&value).is_err());
        let ok = json!({"v": 1, "type": "event", "from": "a", "to": "b", "event": {"type": "agent_settled"}});
        assert!(parse_and_validate(&ok).is_ok());
    }

    #[test]
    fn message_types_require_message() {
        let value = json!({"v": 1, "type": "prompt", "from": "a", "to": "b"});
        assert!(parse_and_validate(&value).is_err());
    }

    #[test]
    fn unknown_type_is_rejected() {
        let value = json!({"v": 1, "type": "custom", "from": "a", "to": "b", "message": "hi"});
        assert!(parse_and_validate(&value).is_err());
    }
}
