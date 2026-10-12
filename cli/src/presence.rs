//! Presence lease parsing for the `sessions` listing.
//!
//! Lease names are produced by the pi extension as
//! `msgq:participant:<kind>:<state>:<id>`; this module is the reader side.

/// Prefix shared by every participant lease.
pub const PRESENCE_PREFIX: &str = "msgq:participant:";

/// One live participant, ready to print.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Roster {
    pub address: String,
    pub kind: String,
    pub state: String,
    pub alias: Option<String>,
    pub expires_at: i64,
}

/// Parse a lease name into `(kind, state, id)`.
///
/// `id` is the trailing remainder, so an id containing `:` still parses.
pub fn parse_lease_name(name: &str) -> Option<(String, String, String)> {
    let rest = name.strip_prefix(PRESENCE_PREFIX)?;
    let mut parts = rest.splitn(3, ':');
    let kind = parts.next()?;
    let state = parts.next()?;
    let id = parts.next()?;
    if kind.is_empty() || state.is_empty() || id.is_empty() {
        return None;
    }
    Some((kind.to_string(), state.to_string(), id.to_string()))
}

/// The alias carried by an owner string (`<alias>#<id>`), if any.
pub fn alias_from_owner(owner: &str) -> Option<&str> {
    let (alias, _) = owner.split_once('#')?;
    let alias = alias.trim();
    if alias.is_empty() { None } else { Some(alias) }
}

/// The address a participant is discoverable at: `<kind>/<alias-or-id>`.
pub fn address_for(kind: &str, id: &str, owner: &str) -> String {
    let self_ref = alias_from_owner(owner).unwrap_or(id);
    format!("{kind}/{self_ref}")
}

/// Sort rank used to de-duplicate by participant: busy, then idle, then blocked.
pub fn state_rank(state: &str) -> u8 {
    match state {
        "busy" => 0,
        "idle" => 1,
        "blocked" => 2,
        _ => 3,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_participant_lease() {
        assert_eq!(
            parse_lease_name("msgq:participant:session:busy:01a127d9"),
            Some(("session".into(), "busy".into(), "01a127d9".into()))
        );
    }

    #[test]
    fn id_is_the_trailing_remainder() {
        assert_eq!(
            parse_lease_name("msgq:participant:cli:idle:a:b:c"),
            Some(("cli".into(), "idle".into(), "a:b:c".into()))
        );
    }

    #[test]
    fn rejects_non_participant_names() {
        for name in [
            "msgq:other:session:busy:x",
            "msgq:participant:session:busy",
            "msgq:participant:session",
            "msgq:participant:session:busy:",
            "nonsense",
        ] {
            assert_eq!(parse_lease_name(name), None, "{name}");
        }
    }

    #[test]
    fn alias_comes_from_the_owner() {
        assert_eq!(alias_from_owner("api-worker#01a127d9"), Some("api-worker"));
        assert_eq!(alias_from_owner("01a127d9"), None);
        assert_eq!(alias_from_owner("#01a127d9"), None);
    }

    #[test]
    fn address_prefers_alias_then_id() {
        assert_eq!(
            address_for("session", "01a127d9", "api-worker#01a127d9"),
            "session/api-worker"
        );
        assert_eq!(
            address_for("session", "01a127d9", "01a127d9"),
            "session/01a127d9"
        );
    }

    #[test]
    fn state_rank_prefers_busy() {
        assert!(state_rank("busy") < state_rank("idle"));
        assert!(state_rank("idle") < state_rank("blocked"));
        assert!(state_rank("blocked") < state_rank("whatever"));
    }
}
