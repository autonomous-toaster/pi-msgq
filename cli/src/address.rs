//! Address grammar and canonicalization.
//!
//! An address identifies both the recipient and the stream it is written to.
//! When it contains a `/`, its prefix must be one of the reserved schemes.
//! A bare name is canonicalized to `group/<name>`.

use crate::error::Error;

/// Reserved address schemes.
pub const RESERVED_SCHEMES: [&str; 4] = ["session", "cwd", "group", "cli"];

/// Canonicalize an address, or return a usage error.
pub fn canonicalize(input: &str) -> Result<String, Error> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Err(Error::usage("address must not be empty"));
    }

    match trimmed.split_once('/') {
        Some((scheme, rest)) => {
            if !RESERVED_SCHEMES.contains(&scheme) {
                return Err(Error::usage(format!(
                    "unknown address scheme '{scheme}'; expected one of session/, cwd/, group/, cli/"
                )));
            }
            if rest.is_empty() {
                return Err(Error::usage(format!(
                    "address scheme '{scheme}/' requires a name"
                )));
            }
            Ok(format!("{scheme}/{rest}"))
        }
        None => Ok(format!("group/{trimmed}")),
    }
}

/// The separator between the address and the offset in a record id.
pub const ID_SEPARATOR: char = '#';

/// Form a record id: the address whose stream holds the record, then its offset.
///
/// An id is derived, never stored: both halves are already known to whoever
/// published the record and to whoever received it.
pub fn record_id(address: &str, offset: i64) -> String {
    format!("{address}{ID_SEPARATOR}{offset}")
}

/// Parse a record id into its canonical address and its offset.
///
/// Split on the *last* separator, because an address's name is unvalidated and
/// may itself contain one.
pub fn parse_record_id(input: &str) -> Result<(String, i64), Error> {
    let trimmed = input.trim();
    let not_an_id = || {
        Error::usage(format!(
            "'{trimmed}' is not a record id; expected <address>{ID_SEPARATOR}<offset>"
        ))
    };
    let (address, offset) = trimmed.rsplit_once(ID_SEPARATOR).ok_or_else(not_an_id)?;
    let offset: i64 = offset.trim().parse().map_err(|_| not_an_id())?;
    if offset < 0 {
        return Err(Error::usage(format!(
            "record id offset must not be negative: '{trimmed}'"
        )));
    }
    Ok((canonicalize(address)?, offset))
}

/// Canonicalize a list of addresses, rejecting duplicates and empties.
pub fn canonicalize_all(addresses: &[String]) -> Result<Vec<String>, Error> {
    if addresses.is_empty() {
        return Err(Error::usage("at least one address is required"));
    }
    let mut out: Vec<String> = Vec::new();
    for a in addresses {
        let canonical = canonicalize(a)?;
        if !out.contains(&canonical) {
            out.push(canonical);
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bare_name_becomes_group() {
        assert_eq!(canonicalize("build.done").unwrap(), "group/build.done");
    }

    #[test]
    fn reserved_schemes_are_kept() {
        assert_eq!(canonicalize("session/api").unwrap(), "session/api");
        assert_eq!(
            canonicalize("cwd/Users-me-proj").unwrap(),
            "cwd/Users-me-proj"
        );
        assert_eq!(canonicalize("group/backend").unwrap(), "group/backend");
        assert_eq!(canonicalize("cli/alice@mac").unwrap(), "cli/alice@mac");
    }

    #[test]
    fn whitespace_is_trimmed() {
        assert_eq!(canonicalize("  session/a  ").unwrap(), "session/a");
    }

    #[test]
    fn unknown_prefix_is_rejected() {
        assert!(canonicalize("foo/bar").is_err());
    }

    #[test]
    fn empty_is_rejected() {
        assert!(canonicalize("   ").is_err());
    }

    #[test]
    fn scheme_only_is_rejected() {
        assert!(canonicalize("session/").is_err());
    }

    #[test]
    fn addresses_are_case_sensitive() {
        assert_ne!(
            canonicalize("session/Api").unwrap(),
            canonicalize("session/api").unwrap()
        );
    }

    #[test]
    fn a_record_id_is_the_address_and_the_offset() {
        assert_eq!(record_id("session/reviewer", 7), "session/reviewer#7");
        assert_eq!(
            parse_record_id("session/reviewer#7").unwrap(),
            ("session/reviewer".to_string(), 7)
        );
    }

    #[test]
    fn a_record_id_round_trips_through_an_address_containing_the_separator() {
        // The default sender contains @ and may contain #: the offset is what
        // follows the last separator.
        let id = record_id("cli/alice@mac", 12);
        assert_eq!(id, "cli/alice@mac#12");
        assert_eq!(
            parse_record_id(&id).unwrap(),
            ("cli/alice@mac".to_string(), 12)
        );
        assert_eq!(
            parse_record_id("session/a#b#3").unwrap(),
            ("session/a#b".to_string(), 3)
        );
    }

    #[test]
    fn a_malformed_record_id_is_rejected() {
        assert!(parse_record_id("session/reviewer").is_err());
        assert!(parse_record_id("session/reviewer#x").is_err());
        assert!(parse_record_id("#7").is_err());
        assert!(parse_record_id("session/reviewer#-1").is_err());
    }
}
