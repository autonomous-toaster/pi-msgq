//! Database path resolution.

use std::path::{Path, PathBuf};

/// Environment variable that overrides the default database path.
pub const ENV_VAR: &str = "MSGQ_DB";

/// The default database path: `~/.pi/agent/msgq/msgq.db`.
pub fn default_db_path() -> PathBuf {
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
    home.join(".pi").join("agent").join("msgq").join("msgq.db")
}

/// Resolve the database path: `--db` > `MSGQ_DB` > default.
pub fn resolve(cli_db: Option<&PathBuf>) -> PathBuf {
    if let Some(p) = cli_db {
        return p.clone();
    }
    if let Ok(v) = std::env::var(ENV_VAR)
        && !v.trim().is_empty()
    {
        return PathBuf::from(v);
    }
    default_db_path()
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
        assert_eq!(resolve(Some(&flag)), flag);
        assert_eq!(resolve(None), PathBuf::from("/tmp/env.db"));

        unsafe { std::env::set_var(ENV_VAR, "   ") };
        assert_eq!(resolve(None), default_db_path());

        unsafe { std::env::remove_var(ENV_VAR) };
        assert_eq!(resolve(None), default_db_path());
    }

    #[test]
    fn default_path_is_under_pi_agent() {
        let path = default_db_path();
        assert!(path.ends_with("msgq/msgq.db"));
    }
}
