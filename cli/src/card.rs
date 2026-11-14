//! Peer locator derivation: the small card a presence record can carry.
//!
//! The card is deliberately flat and small. It answers "where is this peer working?", not "what is
//! this peer?" — see `PROTOCOL.md` and the `msgq-peer-locator` change.

use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::{Map, Value};

use crate::error::Error;

/// How much of the current path to put in the locator.
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum PathMode {
    Relative,
    Full,
    None,
}

impl PathMode {
    /// Parse the CLI spelling. `relative` is the default when the flag is absent.
    pub fn parse(raw: Option<&str>) -> Result<Self, Error> {
        match raw.unwrap_or("relative").trim() {
            "relative" => Ok(Self::Relative),
            "full" => Ok(Self::Full),
            "none" => Ok(Self::None),
            other => Err(Error::usage(format!(
                "'{other}' is not a path mode; expected relative, full, or none"
            ))),
        }
    }
}

/// Derive the locator card for `cwd`.
///
/// A missing git repository or a missing `git` binary is not an error: the card falls back to the
/// directory name and omits the fields git would have supplied.
pub fn derive_card(cwd: &Path, paths: PathMode) -> Result<Value, Error> {
    let cwd = resolve_cwd(cwd)?;
    let mut card = Map::new();

    match git_locator(&cwd) {
        Some(git) => {
            card.insert("project".to_string(), Value::String(git.project));
            if let Some(branch) = git.branch {
                card.insert("branch".to_string(), Value::String(branch));
            }
            card.insert("worktree".to_string(), Value::String(git.worktree));
            if let Some(path) = path_value(&cwd, paths, Some(&git.prefix)) {
                card.insert("path".to_string(), Value::String(path));
            }
        }
        None => {
            let project = basename(&cwd);
            card.insert("project".to_string(), Value::String(project));
            if let Some(path) = path_value(&cwd, paths, None) {
                card.insert("path".to_string(), Value::String(path));
            }
        }
    }

    Ok(Value::Object(card))
}

/// A card as one roster column: `project branch path`, in that order.
///
/// The identity and the display name are already in the address, and the worktree usually repeats
/// the repository name inside `project`, so the column keeps only what tells peers apart. A `path`
/// of `.` means the repository root and is left out; a card with nothing to say is `-`.
pub fn summary(card: &Value) -> String {
    let field = |key: &str| {
        card.get(key)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|v| !v.is_empty())
    };
    let mut parts: Vec<&str> = Vec::new();
    for key in ["project", "branch"] {
        if let Some(value) = field(key) {
            parts.push(value);
        }
    }
    if let Some(path) = field("path").filter(|path| *path != ".") {
        parts.push(path);
    }
    if parts.is_empty() {
        "-".to_string()
    } else {
        parts.join("  ")
    }
}

/// Normalize a git remote URL to `host/owner/repo`.
///
/// Both `https://host/owner/repo.git` and `git@host:owner/repo.git` become `host/owner/repo`. A
/// remote that does not look like a URL is returned with only its slashes and `.git` suffix trimmed.
pub fn normalize_remote(raw: &str) -> String {
    let mut value = raw.trim().to_string();
    if value.is_empty() {
        return String::new();
    }

    for scheme in ["https://", "http://", "ssh://", "git://"] {
        if let Some(rest) = value.strip_prefix(scheme) {
            value = rest.to_string();
            break;
        }
    }

    // Strip userinfo, including the common `git@` form and the scp-like `git@host:path`.
    if let Some((user, rest)) = value.split_once('@')
        && !user.contains('/')
    {
        value = rest.to_string();
    }

    if let Some((host, path)) = value.split_once(':')
        && !host.contains('/')
    {
        value = format!("{host}/{path}");
    }

    value
        .trim_start_matches('/')
        .trim_end_matches('/')
        .trim_end_matches(".git")
        .trim_end_matches('/')
        .to_string()
}

struct GitLocator {
    project: String,
    branch: Option<String>,
    worktree: String,
    prefix: String,
}

fn git_locator(cwd: &Path) -> Option<GitLocator> {
    let inside = git(cwd, &["rev-parse", "--is-inside-work-tree"])?;
    if inside.trim() != "true" {
        return None;
    }

    let root = git(cwd, &["rev-parse", "--show-toplevel"])?;
    let root = root.trim();
    if root.is_empty() {
        return None;
    }
    let root = Path::new(root);
    let worktree = basename(root);

    let project = git(cwd, &["remote", "get-url", "origin"])
        .map(|remote| normalize_remote(&remote))
        .filter(|remote| !remote.is_empty())
        .unwrap_or_else(|| worktree.clone());

    let branch = git(cwd, &["branch", "--show-current"])
        .map(|branch| branch.trim().to_string())
        .filter(|branch| !branch.is_empty());

    let prefix = git(cwd, &["rev-parse", "--show-prefix"])
        .unwrap_or_default()
        .trim()
        .trim_end_matches('/')
        .to_string();

    Some(GitLocator {
        project,
        branch,
        worktree,
        prefix,
    })
}

fn path_value(cwd: &Path, mode: PathMode, prefix: Option<&str>) -> Option<String> {
    match mode {
        PathMode::None => None,
        PathMode::Full => Some(cwd.to_string_lossy().into_owned()),
        PathMode::Relative => prefix.map(|p| {
            if p.is_empty() {
                ".".to_string()
            } else {
                p.to_string()
            }
        }),
    }
}

fn resolve_cwd(cwd: &Path) -> Result<PathBuf, Error> {
    if cwd.as_os_str().is_empty() {
        return std::env::current_dir().map_err(|e| Error::usage(format!("cannot read cwd: {e}")));
    }
    if !cwd.exists() {
        return Err(Error::usage(format!(
            "{} does not exist",
            cwd.to_string_lossy()
        )));
    }
    if !cwd.is_dir() {
        return Err(Error::usage(format!(
            "{} is not a directory",
            cwd.to_string_lossy()
        )));
    }
    cwd.canonicalize()
        .map_err(|e| Error::usage(format!("cannot resolve {}: {e}", cwd.to_string_lossy())))
}

fn basename(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.to_string_lossy().into_owned())
}

fn git(cwd: &Path, args: &[&str]) -> Option<String> {
    let out = Command::new("git")
        .arg("-C")
        .arg(cwd)
        .args(args)
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    String::from_utf8(out.stdout).ok()
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::sync::atomic::{AtomicU64, Ordering};

    use super::*;

    fn temp(name: &str) -> PathBuf {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let unique = COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir =
            std::env::temp_dir().join(format!("msgq-card-{name}-{}-{unique}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn git_in(dir: &Path, args: &[&str]) {
        let out = Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(args)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "git {args:?} failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }

    fn init_repo(name: &str) -> PathBuf {
        let dir = temp(name);
        git_in(&dir, &["init", "-b", "main"]);
        git_in(&dir, &["config", "user.email", "test@example.invalid"]);
        git_in(&dir, &["config", "user.name", "Test"]);
        fs::write(dir.join("file.txt"), "one\n").unwrap();
        git_in(&dir, &["add", "."]);
        git_in(&dir, &["commit", "-m", "init"]);
        dir
    }

    fn value<'a>(card: &'a Value, key: &str) -> Option<&'a str> {
        card.get(key).and_then(Value::as_str)
    }

    #[test]
    fn normalizes_https_and_ssh_remotes() {
        assert_eq!(
            normalize_remote("https://github.com/org/repo.git"),
            "github.com/org/repo"
        );
        assert_eq!(
            normalize_remote("git@github.com:org/repo.git"),
            "github.com/org/repo"
        );
        assert_eq!(
            normalize_remote("ssh://git@github.com/org/repo.git"),
            "github.com/org/repo"
        );
        assert_eq!(
            normalize_remote("git://github.com/org/repo"),
            "github.com/org/repo"
        );
        assert_eq!(normalize_remote("  "), "");
    }

    #[test]
    fn derives_a_git_locator() {
        let dir = init_repo("git");
        git_in(
            &dir,
            &["remote", "add", "origin", "git@github.com:org/repo.git"],
        );
        let card = derive_card(&dir, PathMode::Relative).unwrap();

        assert_eq!(value(&card, "project"), Some("github.com/org/repo"));
        assert_eq!(value(&card, "branch"), Some("main"));
        assert_eq!(
            value(&card, "worktree"),
            Some(dir.file_name().unwrap().to_string_lossy().as_ref())
        );
        assert_eq!(value(&card, "path"), Some("."));
        assert_eq!(card.as_object().unwrap().len(), 4);
    }

    #[test]
    fn derives_a_subdirectory_path() {
        let dir = init_repo("subdir");
        let sub = dir.join("src").join("api");
        fs::create_dir_all(&sub).unwrap();
        let card = derive_card(&sub, PathMode::Relative).unwrap();

        assert_eq!(
            value(&card, "worktree"),
            Some(dir.file_name().unwrap().to_string_lossy().as_ref())
        );
        assert_eq!(value(&card, "path"), Some("src/api"));
        assert_eq!(value(&card, "branch"), Some("main"));
    }

    #[test]
    fn falls_back_outside_git() {
        let dir = temp("plain");
        let card = derive_card(&dir, PathMode::Relative).unwrap();

        let name = dir.file_name().unwrap().to_string_lossy();
        assert_eq!(value(&card, "project"), Some(name.as_ref()));
        assert_eq!(value(&card, "branch"), None);
        assert_eq!(value(&card, "worktree"), None);
        assert_eq!(value(&card, "path"), None);
    }

    #[test]
    fn full_and_none_path_modes() {
        let dir = init_repo("modes");
        let full = derive_card(&dir, PathMode::Full).unwrap();
        assert_eq!(
            value(&full, "path"),
            Some(dir.canonicalize().unwrap().to_string_lossy().as_ref())
        );

        let none = derive_card(&dir, PathMode::None).unwrap();
        assert_eq!(value(&none, "path"), None);
    }

    #[test]
    fn omits_branch_on_a_detached_head() {
        let dir = init_repo("detached");
        git_in(&dir, &["checkout", "--detach", "HEAD"]);
        let card = derive_card(&dir, PathMode::Relative).unwrap();
        assert_eq!(value(&card, "branch"), None);
        assert!(value(&card, "project").is_some());
    }

    #[test]
    fn reports_a_linked_worktree_by_its_directory_name() {
        let dir = init_repo("main-repo");
        let linked = temp("linked-worktree");
        let linked_path = linked.join("review");
        git_in(
            &dir,
            &[
                "worktree",
                "add",
                "-b",
                "review",
                linked_path.to_string_lossy().as_ref(),
            ],
        );
        let card = derive_card(&linked_path, PathMode::Relative).unwrap();
        assert_eq!(value(&card, "worktree"), Some("review"));
        assert_eq!(value(&card, "branch"), Some("review"));
    }

    #[test]
    fn summarizes_a_card_as_one_locator_column() {
        let dir = init_repo("summary");
        git_in(
            &dir,
            &["remote", "add", "origin", "git@github.com:org/repo.git"],
        );
        let card = derive_card(&dir, PathMode::Relative).unwrap();

        assert_eq!(summary(&card), "github.com/org/repo  main");
    }

    #[test]
    fn a_summary_names_a_subdirectory_and_skips_the_worktree() {
        let dir = init_repo("summary-sub");
        git_in(
            &dir,
            &["remote", "add", "origin", "git@github.com:org/repo.git"],
        );
        let sub = dir.join("src").join("api");
        fs::create_dir_all(&sub).unwrap();
        let card = derive_card(&sub, PathMode::Relative).unwrap();

        assert_eq!(summary(&card), "github.com/org/repo  main  src/api");
        assert!(
            !summary(&card).contains(card["worktree"].as_str().unwrap()),
            "the worktree is not a column of its own"
        );
    }

    #[test]
    fn a_summary_without_a_card_is_a_placeholder() {
        for card in [
            serde_json::json!({}),
            serde_json::json!({"path": "."}),
            serde_json::json!({"head": "ignored", "project": "  "}),
            serde_json::json!("not an object"),
        ] {
            assert_eq!(summary(&card), "-", "{card}");
        }
    }

    #[test]
    fn a_full_path_is_shown_and_a_dot_path_is_not() {
        assert_eq!(
            summary(&serde_json::json!({"project": "org/repo", "path": "/home/dev/repo"})),
            "org/repo  /home/dev/repo"
        );
        assert_eq!(
            summary(&serde_json::json!({"project": "org/repo", "path": "."})),
            "org/repo"
        );
    }
}
