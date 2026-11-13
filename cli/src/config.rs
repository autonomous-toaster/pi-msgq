//! Database path resolution.

use std::path::{Path, PathBuf};

use crate::error::Error;

/// Environment variable that overrides the default database path.
pub const ENV_VAR: &str = "MSGQ_DB";

/// Environment variable that overrides the retention window.
pub const RETENTION_ENV: &str = "MSGQ_RETENTION";

/// The retention window used when nothing else says otherwise, in seconds.
pub const DEFAULT_RETENTION_S: i64 = 3_600;

/// Parse an age such as `7d`, `2h`, `30m` or `45s` into seconds.
pub fn parse_age(raw: &str) -> Result<i64, Error> {
    let trimmed = raw.trim();
    let (digits, unit) = if let Some(rest) = trimmed.strip_suffix('d') {
        (rest, 86_400)
    } else if let Some(rest) = trimmed.strip_suffix('h') {
        (rest, 3_600)
    } else if let Some(rest) = trimmed.strip_suffix('m') {
        (rest, 60)
    } else if let Some(rest) = trimmed.strip_suffix('s') {
        (rest, 1)
    } else {
        return Err(Error::usage(format!(
            "'{trimmed}' is not an age; expected a number and one of d, h, m, s (for example 7d)"
        )));
    };
    let count: i64 = digits.trim().parse().map_err(|_| {
        Error::usage(format!(
            "'{trimmed}' is not an age; expected a number and one of d, h, m, s (for example 7d)"
        ))
    })?;
    if count <= 0 {
        return Err(Error::usage(format!("'{trimmed}' must be a positive age")));
    }
    Ok(count * unit)
}

/// Resolve the retention window in seconds: `--retention`/`--older-than`, then `MSGQ_RETENTION`,
/// then one hour.
pub fn resolve_retention(cli: Option<&str>) -> Result<i64, Error> {
    let explicit = cli
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .or_else(|| {
            std::env::var(RETENTION_ENV)
                .ok()
                .map(|value| value.trim().to_string())
                .filter(|value| !value.is_empty())
        });
    match explicit {
        Some(raw) => parse_age(&raw),
        None => Ok(DEFAULT_RETENTION_S),
    }
}

/// The default database path: `~/.pi/agent/msgq/msgq.db`.
pub fn default_db_path() -> PathBuf {
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
    home.join(".pi").join("agent").join("msgq").join("msgq.db")
}

/// Where the bus lives: a local file, or a shared server.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    /// A filesystem path, served by the local engine.
    Local(PathBuf),
    /// A server connection string, served by the shared engine.
    Shared(String),
}

/// Resolve the bus target: the flag, then the environment, then the default path; the form of the
/// location decides the engine.
///
/// A server connection string selects the shared engine; anything else is a filesystem path and
/// selects the local engine. Exactly one engine serves one invocation, and an unimplemented scheme
/// is a usage error rather than a filename.
pub fn resolve_target(cli_db: Option<&PathBuf>) -> Result<Target, Error> {
    let raw = match cli_db {
        Some(path) => path.display().to_string(),
        None => match std::env::var(ENV_VAR) {
            Ok(value) if !value.trim().is_empty() => value.trim().to_string(),
            _ => default_db_path().display().to_string(),
        },
    };
    target_from(&raw)
}

/// Classify one location: a connection string or a path.
fn target_from(raw: &str) -> Result<Target, Error> {
    match raw.split_once("://") {
        Some((scheme, rest)) => match scheme {
            "postgres" | "postgresql" if !rest.trim().is_empty() => {
                Ok(Target::Shared(raw.to_string()))
            }
            "postgres" | "postgresql" => Err(Error::usage(
                "a connection string needs a host: expected postgres://user@host/db",
            )),
            other => Err(Error::usage(format!(
                "unsupported bus scheme '{other}'; expected postgres:// or a filesystem path"
            ))),
        },
        None => Ok(Target::Local(PathBuf::from(raw))),
    }
}

/// Human-readable rendering of a path.
pub fn display(path: &Path) -> String {
    path.display().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    // Environment variables are process-global, so precedence is checked in a
    // single sequential test rather than several racing ones.
    #[test]
    fn resolves_path_precedence() {
        let flag = PathBuf::from("/tmp/flag.db");

        unsafe { std::env::set_var(ENV_VAR, "/tmp/env.db") };
        assert_eq!(
            resolve_target(Some(&flag)).unwrap(),
            Target::Local(flag.clone())
        );
        assert_eq!(
            resolve_target(None).unwrap(),
            Target::Local(PathBuf::from("/tmp/env.db"))
        );

        unsafe { std::env::set_var(ENV_VAR, "   ") };
        assert_eq!(
            resolve_target(None).unwrap(),
            Target::Local(default_db_path())
        );

        unsafe { std::env::remove_var(ENV_VAR) };
        assert_eq!(
            resolve_target(None).unwrap(),
            Target::Local(default_db_path())
        );
    }

    #[test]
    fn a_connection_string_selects_the_shared_engine() {
        let target = resolve_target(Some(&PathBuf::from("postgres://bus@host/db"))).unwrap();
        assert_eq!(target, Target::Shared("postgres://bus@host/db".to_string()));
        // A connection string never becomes a filename.
        assert!(!matches!(target, Target::Local(_)));
    }

    #[test]
    fn an_unsupported_scheme_is_a_usage_error() {
        let err = resolve_target(Some(&PathBuf::from("mysql://host/db"))).unwrap_err();
        assert!(err.to_string().contains("unsupported bus scheme"), "{err}");
        let bare = resolve_target(Some(&PathBuf::from("postgres://"))).unwrap_err();
        assert!(bare.to_string().contains("needs a host"), "{bare}");
    }

    #[test]
    fn default_path_is_under_pi_agent() {
        let path = default_db_path();
        assert!(path.ends_with("msgq/msgq.db"));
    }

    #[test]
    fn parses_the_ages_the_documentation_shows() {
        assert_eq!(parse_age("7d").unwrap(), 7 * 86_400);
        assert_eq!(parse_age("2h").unwrap(), 7_200);
        assert_eq!(parse_age("30m").unwrap(), 1_800);
        assert_eq!(parse_age("45s").unwrap(), 45);
        assert!(parse_age("45").is_err());
        assert!(parse_age("0h").is_err());
    }

    // The environment is process-global, so the retention precedence is checked in one sequential
    // test rather than several racing ones.
    #[test]
    fn resolves_the_retention_window_precedence() {
        unsafe { std::env::remove_var(RETENTION_ENV) };
        assert_eq!(resolve_retention(None).unwrap(), DEFAULT_RETENTION_S);

        unsafe { std::env::set_var(RETENTION_ENV, "15m") };
        assert_eq!(resolve_retention(None).unwrap(), 900);
        assert_eq!(resolve_retention(Some("2h")).unwrap(), 7_200);

        unsafe { std::env::set_var(RETENTION_ENV, "   ") };
        assert_eq!(resolve_retention(None).unwrap(), DEFAULT_RETENTION_S);

        unsafe { std::env::set_var(RETENTION_ENV, "forever") };
        assert!(resolve_retention(None).is_err());

        unsafe { std::env::remove_var(RETENTION_ENV) };
        assert_eq!(resolve_retention(None).unwrap(), DEFAULT_RETENTION_S);
    }
}
