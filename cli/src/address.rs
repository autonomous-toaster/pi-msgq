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
}
