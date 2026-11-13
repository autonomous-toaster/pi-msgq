//! Database path resolution.

use std::path::{Path, PathBuf};

use crate::error::Error;

/// Environment variable that overrides the default database path.
/// The location of a local bus: a filesystem path.
///
/// A connection string is still accepted here, so a script that already points `MSGQ_DB` at a
/// server keeps working, but a deployment should name the two apart: a path here, a URI below.
pub const ENV_VAR: &str = "MSGQ_DB";

/// The location of a shared bus: a connection string.
pub const URI_ENV: &str = "MSGQ_DB_URI";

/// The credential for a shared bus: the environment first, then the identity file.
pub const TOKEN_ENV: &str = "MSGQ_TOKEN";

/// The scope this session belongs to. Presence and the roster are per scope, so two teams on one
/// bus do not see each other's participants.
pub const SCOPE_ENV: &str = "MSGQ_SCOPE";

/// The configured scope, if any.
pub fn scope() -> Option<String> {
    let scope = std::env::var(SCOPE_ENV).ok()?;
    let scope = scope.trim().to_string();
    if scope.is_empty() { None } else { Some(scope) }
}

/// Where a credential may be kept instead of in the environment. A deployment that can avoid
/// leaving a secret in the process table does so.
pub fn identity_file() -> Option<PathBuf> {
    std::env::var("HOME")
        .ok()
        .map(|home| PathBuf::from(home).join(".config/msgq/token"))
}

/// The credential to present, if one is configured.
pub fn credential() -> Option<String> {
    if let Ok(token) = std::env::var(TOKEN_ENV) {
        let token = token.trim();
        if !token.is_empty() {
            return Some(token.to_string());
        }
    }
    let path = identity_file()?;
    let token = std::fs::read_to_string(path).ok()?;
    let token = token.trim();
    if token.is_empty() {
        None
    } else {
        Some(token.to_string())
    }
}

/// The PostgreSQL settings this bus reads: libpq's names, under this bus's own prefix.
///
/// `msgq` is the client here, so the settings belong to it and are named for it. The suffix is
/// libpq's spelling, because that is the spelling an operator already knows. A shell's global
/// `PG*` settings are deliberately not read: they belong to whatever else is running, and a message
/// bus that silently connected to the wrong database would be worse than one that asks.
///
/// Each entry maps a variable to the libpq keyword it stands for.
pub const PG_ENV: [(&str, &str); 6] = [
    ("MSGQ_PGHOST", "host"),
    ("MSGQ_PGPORT", "port"),
    ("MSGQ_PGUSER", "user"),
    ("MSGQ_PGPASSWORD", "password"),
    ("MSGQ_PGDATABASE", "dbname"),
    ("MSGQ_PGSSLMODE", "sslmode"),
];

/// The `MSGQ_PG*` settings that are present, in libpq keyword order.
pub fn pg_settings() -> Vec<(&'static str, String)> {
    PG_ENV
        .iter()
        .filter_map(|(variable, keyword)| {
            let value = std::env::var(variable).ok()?;
            let value = value.trim();
            if value.is_empty() {
                None
            } else {
                Some((*keyword, value.to_string()))
            }
        })
        .collect()
}

/// Whether the environment names a server at all. A password alone does not select the shared
/// engine; a host or a database does.
pub fn pg_env_names_a_server() -> bool {
    pg_settings()
        .iter()
        .any(|(keyword, _)| matches!(*keyword, "host" | "dbname"))
}

/// Apply the `MSGQ_PG*` settings to a connection configuration.
///
/// They win over anything the connection string said, so a URI can name the server while the
/// secret stays in the environment and out of the URI.
pub fn apply_pg_env(config: &mut postgres::Config) -> Result<(), Error> {
    apply_pg_settings(config, pg_settings())
}

/// The same mapping, applied to settings a caller already has. Separated so it can be tested
/// without touching the process environment.
pub fn apply_pg_settings(
    config: &mut postgres::Config,
    settings: Vec<(&'static str, String)>,
) -> Result<(), Error> {
    // The variables supply what the location does not, which is how libpq treats PG*: a host named
    // in the connection string is not overwritten by PGHOST, but a missing password is filled in.
    for (keyword, value) in settings {
        match keyword {
            "host" if config.get_hosts().is_empty() => {
                config.host(&value);
            }
            "port" if config.get_ports().is_empty() => {
                let port: u16 = value.parse().map_err(|_| {
                    Error::usage(format!("MSGQ_PGPORT is not a port number: {value}"))
                })?;
                config.port(port);
            }
            "user" if config.get_user().is_none() => {
                config.user(&value);
            }
            "password" if config.get_password().is_none() => {
                config.password(&value);
            }
            "dbname" if config.get_dbname().is_none() => {
                config.dbname(&value);
            }
            // Accepted and reported rather than silently ignored: this build connects without TLS,
            // so asking for it would be a promise the bus cannot keep.
            "sslmode" if !matches!(value.as_str(), "disable" | "allow" | "prefer") => {
                return Err(Error::usage(format!(
                    "MSGQ_PGSSLMODE={value} needs a TLS build of the bus; this build connects without TLS"
                )));
            }
            _ => {}
        }
    }
    Ok(())
}

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
    let chosen = choose_location(
        cli_db.map(|path| path.display().to_string()),
        std::env::var(URI_ENV).ok(),
        std::env::var(ENV_VAR).ok(),
    );
    match chosen {
        Some(raw) => target_from(&raw),
        // Nothing named a location, but the environment names a server: that is a choice too.
        None if pg_env_names_a_server() => Ok(Target::Shared(String::new())),
        None => Ok(Target::Local(default_db_path())),
    }
}

/// Which location wins: the flag, then the server's URI, then the file.
///
/// The server comes first because a deployment that sets both has said where its bus is, and the
/// file variable is the older of the two.
fn choose_location(
    flag: Option<String>,
    uri: Option<String>,
    file: Option<String>,
) -> Option<String> {
    let non_empty = |value: Option<String>| {
        value
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
    };
    non_empty(flag)
        .or_else(|| non_empty(uri))
        .or_else(|| non_empty(file))
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

impl Target {
    /// The file backing a local bus, or nothing when the bus is shared.
    pub fn local_path(&self) -> Option<&Path> {
        match self {
            Target::Local(path) => Some(path),
            Target::Shared(_) => None,
        }
    }

    /// How to name the bus in output. A connection string is never printed: it may carry a
    /// password, and a printed password cannot be unprinted.
    pub fn display(&self) -> String {
        match self {
            Target::Local(path) => crate::config::display(path),
            Target::Shared(dsn) if dsn.trim().is_empty() => {
                // Named by the environment: report the parts, never the secret.
                let host = std::env::var("MSGQ_PGHOST").unwrap_or_else(|_| "localhost".into());
                let database = std::env::var("MSGQ_PGDATABASE").unwrap_or_default();
                if database.is_empty() {
                    format!("postgres://{host}")
                } else {
                    format!("postgres://{host}/{database}")
                }
            }
            Target::Shared(dsn) if !dsn.contains("://") => {
                // A libpq keyword string: report host and database, and nothing else.
                let part = |keyword: &str| {
                    dsn.split_whitespace()
                        .find_map(|pair| pair.strip_prefix(&format!("{keyword}=")))
                        .map(str::to_string)
                };
                match part("dbname") {
                    Some(database) => format!(
                        "postgres://{}/{}",
                        part("host").unwrap_or_else(|| "localhost".to_string()),
                        database
                    ),
                    None => format!(
                        "postgres://{}",
                        part("host").unwrap_or_else(|| "localhost".to_string())
                    ),
                }
            }
            Target::Shared(dsn) => {
                let after_scheme = dsn.split_once("://").map(|(_, rest)| rest).unwrap_or(dsn);
                let authority = after_scheme.split(['/', '?']).next().unwrap_or("");
                let host = authority.rsplit('@').next().unwrap_or(authority);
                let database = after_scheme
                    .split_once('/')
                    .map(|(_, rest)| rest.split('?').next().unwrap_or(rest))
                    .unwrap_or("");
                if database.is_empty() {
                    format!("postgres://{host}")
                } else {
                    format!("postgres://{host}/{database}")
                }
            }
        }
    }
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

#[cfg(test)]
mod pg_env_tests {
    use super::*;

    fn settings(pairs: &[(&'static str, &str)]) -> Vec<(&'static str, String)> {
        pairs
            .iter()
            .map(|(key, value)| (*key, (*value).to_string()))
            .collect()
    }

    #[test]
    fn the_server_uri_wins_over_the_file_and_the_flag_wins_over_both() {
        let uri = Some("postgres://bus@db/msgq".to_string());
        let file = Some("/tmp/msgq.db".to_string());
        assert_eq!(
            choose_location(Some("/tmp/flag.db".to_string()), uri.clone(), file.clone()),
            Some("/tmp/flag.db".to_string()),
            "the flag is the most specific thing an operator can say"
        );
        assert_eq!(
            choose_location(None, uri.clone(), file.clone()),
            Some("postgres://bus@db/msgq".to_string()),
            "a deployment that sets both has said where its bus is"
        );
        assert_eq!(choose_location(None, None, file.clone()), file);
        assert_eq!(
            choose_location(Some("  ".to_string()), Some(String::new()), file.clone()),
            file,
            "an empty value is not an answer"
        );
    }

    #[test]
    fn the_pg_variables_fill_what_the_location_does_not() {
        let mut config: postgres::Config = "postgres://someone@db.example:6000/other"
            .parse()
            .expect("a URI");
        apply_pg_settings(
            &mut config,
            settings(&[
                ("host", "127.0.0.1"),
                ("port", "55433"),
                ("user", "msgq_runtime"),
                ("password", "secret"),
                ("dbname", "msgq"),
            ]),
        )
        .expect("the settings apply");
        assert_eq!(
            config.get_hosts().len(),
            1,
            "a host in the location is not overwritten"
        );
        assert_eq!(config.get_ports(), &[6000]);
        assert_eq!(config.get_user(), Some("someone"));
        assert_eq!(config.get_dbname(), Some("other"));
        assert_eq!(
            config.get_password(),
            Some(b"secret".as_slice()),
            "a missing secret is supplied"
        );
    }

    #[test]
    fn the_pg_variables_are_enough_on_their_own() {
        let mut config = postgres::Config::new();
        apply_pg_settings(
            &mut config,
            settings(&[
                ("host", "127.0.0.1"),
                ("port", "55433"),
                ("user", "msgq"),
                ("dbname", "msgq"),
            ]),
        )
        .expect("the settings apply");
        assert_eq!(config.get_hosts().len(), 1);
        assert_eq!(config.get_ports(), &[55433]);
        assert_eq!(config.get_user(), Some("msgq"));
        assert_eq!(config.get_dbname(), Some("msgq"));
    }

    #[test]
    fn a_bad_port_is_a_usage_error_rather_than_a_default() {
        let mut config = postgres::Config::new();
        let failure = apply_pg_settings(&mut config, settings(&[("port", "not-a-port")]))
            .expect_err("a malformed port cannot be guessed");
        assert!(failure.to_string().contains("MSGQ_PGPORT"));
    }

    #[test]
    fn asking_for_tls_is_refused_rather_than_ignored() {
        let mut config = postgres::Config::new();
        let failure = apply_pg_settings(&mut config, settings(&[("sslmode", "require")]))
            .expect_err("this build cannot honour require");
        assert!(failure.to_string().contains("without TLS"));

        // The permissive modes are accepted: they describe what this build already does.
        for mode in ["disable", "allow", "prefer"] {
            apply_pg_settings(&mut config, settings(&[("sslmode", mode)]))
                .expect("a permissive mode is honest");
        }
    }

    #[test]
    fn a_secret_never_reaches_the_displayed_location() {
        let target = Target::Shared("host=db.example dbname=msgq password=hunter2".to_string());
        let shown = target.display();
        assert_eq!(shown, "postgres://db.example/msgq");
        assert!(!shown.contains("hunter2"), "{shown}");

        let uri = Target::Shared("postgres://bus:hunter2@db.example/msgq".to_string());
        assert_eq!(uri.display(), "postgres://db.example/msgq");
        assert!(!uri.display().contains("hunter2"));

        let by_env = Target::Shared(String::new());
        assert!(by_env.display().starts_with("postgres://"));
    }
}
