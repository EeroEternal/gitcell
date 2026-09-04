//! Thin wrappers around common git operations, scoped by repository name.
//!
//! gitcell does not reimplement git; it shells out to the system `git`
//! binary underneath each managed repository's working directory.

use std::path::{Path, PathBuf};
use std::process::Command;

use crate::error::{Error, Result};

const IDENTITY_NAME: &str = "gitcell";
const IDENTITY_EMAIL: &str = "gitcell@localhost";

/// Validate a repository name to prevent path traversal or escaping the
/// configured data directory. Only ASCII alphanumerics, `-`, and `_` are
/// allowed, and the name must be non-empty.
pub fn validate_repo_name(name: &str) -> Result<()> {
    if name.is_empty() {
        return Err(Error::InvalidRequest(
            "repository name must not be empty".into(),
        ));
    }
    let valid = name
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
    if !valid || name == "." || name == ".." {
        return Err(Error::InvalidRequest(format!(
            "invalid repository name: {name:?} (only alphanumerics, '-', and '_' are allowed)"
        )));
    }
    Ok(())
}

/// Validate a git ref (branch name or SHA-ish token).
pub fn validate_ref(name: &str) -> Result<()> {
    if name.is_empty() || name == "." || name == ".." || name.starts_with('-') {
        return Err(Error::InvalidRequest(format!("invalid git ref: {name:?}")));
    }
    if name.contains("..") || name.contains('\0') {
        return Err(Error::InvalidRequest(format!("invalid git ref: {name:?}")));
    }
    let valid = name
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '/' | '.'));
    if !valid {
        return Err(Error::InvalidRequest(format!("invalid git ref: {name:?}")));
    }
    Ok(())
}

/// Resolve the on-disk path for a given repository name under `data_dir`,
/// rejecting names that would escape the data directory.
pub fn repo_path(data_dir: &Path, name: &str) -> Result<PathBuf> {
    validate_repo_name(name)?;
    Ok(data_dir.join(name))
}

#[derive(Debug, Clone)]
pub struct GitOutput {
    pub success: bool,
    pub stdout: String,
    pub stderr: String,
}

fn run(path: &Path, args: &[&str]) -> Result<GitOutput> {
    let output = Command::new("git")
        .args(args)
        .current_dir(path)
        .output()
        .map_err(|e| Error::Git(format!("failed to spawn git: {e}")))?;

    Ok(GitOutput {
        success: output.status.success(),
        stdout: String::from_utf8_lossy(&output.stdout).trim().to_string(),
        stderr: String::from_utf8_lossy(&output.stderr).trim().to_string(),
    })
}

fn require_ok(result: GitOutput) -> Result<GitOutput> {
    if result.success {
        Ok(result)
    } else {
        let msg = if result.stderr.is_empty() {
            result.stdout
        } else if result.stdout.is_empty() {
            result.stderr
        } else {
            format!("{}\n{}", result.stderr, result.stdout)
        };
        Err(Error::Git(msg))
    }
}

/// Set a local git identity when the repo has none, so commits work out of the box.
pub fn ensure_identity(path: &Path) -> Result<()> {
    let email = run(path, &["config", "--get", "user.email"])?;
    if email.success && !email.stdout.is_empty() {
        return Ok(());
    }
    require_ok(run(path, &["config", "user.email", IDENTITY_EMAIL])?)?;
    require_ok(run(path, &["config", "user.name", IDENTITY_NAME])?)?;
    Ok(())
}

/// Initialize a new git repository at `path`, creating the directory if
/// needed. Uses `main` as the default branch and installs a local identity.
pub fn init(path: &Path) -> Result<GitOutput> {
    std::fs::create_dir_all(path)
        .map_err(|e| Error::Git(format!("failed to create repo directory: {e}")))?;
    let result = require_ok(run(path, &["init", "-b", "main"])?)?;
    ensure_identity(path)?;
    Ok(result)
}

pub fn list_repos(data_dir: &Path) -> Result<Vec<String>> {
    if !data_dir.exists() {
        return Ok(vec![]);
    }
    let mut names = Vec::new();
    let entries = std::fs::read_dir(data_dir)
        .map_err(|e| Error::Git(format!("failed to read {}: {e}", data_dir.display())))?;
    for entry in entries {
        let entry = entry.map_err(|e| Error::Git(e.to_string()))?;
        let file_type = entry.file_type().map_err(|e| Error::Git(e.to_string()))?;
        if !file_type.is_dir() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().to_string();
        if validate_repo_name(&name).is_ok() && is_git_repo(&entry.path()) {
            names.push(name);
        }
    }
    names.sort();
    Ok(names)
}

pub fn status(path: &Path) -> Result<String> {
    Ok(require_ok(run(path, &["status", "--short", "--branch"])?)?.stdout)
}

/// Working tree + index vs HEAD. Before the first commit, falls back to unstaged diff.
pub fn diff(path: &Path) -> Result<String> {
    let against_head = run(path, &["diff", "--no-color", "HEAD"])?;
    if against_head.success {
        return Ok(against_head.stdout);
    }
    Ok(require_ok(run(path, &["diff", "--no-color"])?)?.stdout)
}

pub fn add(path: &Path, paths: &[String]) -> Result<GitOutput> {
    let targets: Vec<&str> = if paths.is_empty() {
        vec!["."]
    } else {
        paths.iter().map(|s| s.as_str()).collect()
    };
    let mut args = vec!["add"];
    args.extend(targets);
    require_ok(run(path, &args)?)
}

pub fn commit(path: &Path, message: &str) -> Result<GitOutput> {
    ensure_identity(path)?;
    require_ok(run(path, &["commit", "-m", message])?)
}

pub fn log(path: &Path, limit: u32) -> Result<String> {
    let limit_arg = format!("-{limit}");
    Ok(require_ok(run(
        path,
        &[
            "log",
            &limit_arg,
            "--pretty=format:%h %ad %s",
            "--date=short",
        ],
    )?)?
    .stdout)
}

pub fn head_sha(path: &Path) -> Result<String> {
    Ok(require_ok(run(path, &["rev-parse", "HEAD"])?)?.stdout)
}

pub fn current_branch(path: &Path) -> Result<String> {
    Ok(require_ok(run(path, &["rev-parse", "--abbrev-ref", "HEAD"])?)?.stdout)
}

pub fn branches(path: &Path) -> Result<Vec<String>> {
    let output = require_ok(run(path, &["branch", "--format=%(refname:short)"])?)?;
    if output.stdout.is_empty() {
        return Ok(vec![]);
    }
    let mut names: Vec<String> = output
        .stdout
        .lines()
        .map(|l| l.trim().to_string())
        .filter(|l| !l.is_empty())
        .collect();
    names.sort();
    Ok(names)
}

pub fn create_branch(path: &Path, name: &str) -> Result<()> {
    validate_ref(name)?;
    require_ok(run(path, &["branch", name])?)?;
    Ok(())
}

pub fn checkout(path: &Path, name: &str) -> Result<()> {
    validate_ref(name)?;
    require_ok(run(path, &["checkout", name])?)?;
    Ok(())
}

pub fn show(path: &Path, rev: &str) -> Result<String> {
    validate_ref(rev)?;
    Ok(require_ok(run(path, &["show", "--stat", "--format=fuller", rev])?)?.stdout)
}

pub fn is_git_repo(path: &Path) -> bool {
    if !path.exists() {
        return false;
    }
    matches!(
        run(path, &["rev-parse", "--is-inside-work-tree"]),
        Ok(GitOutput { success: true, stdout, .. }) if stdout == "true"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_invalid_repo_names() {
        assert!(validate_repo_name("").is_err());
        assert!(validate_repo_name("..").is_err());
        assert!(validate_repo_name("../escape").is_err());
        assert!(validate_repo_name("a/b").is_err());
        assert!(validate_repo_name("valid-repo_1").is_ok());
    }

    #[test]
    fn repo_path_rejects_traversal() {
        let data_dir = Path::new("/tmp/gitcell-data");
        assert!(repo_path(data_dir, "../../etc").is_err());
        assert!(repo_path(data_dir, "my-repo").is_ok());
    }

    #[test]
    fn init_creates_main_and_commit_works_without_manual_identity() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path().join("repo");
        init(&repo).unwrap();
        assert!(is_git_repo(&repo));
        std::fs::write(repo.join("a.txt"), "a\n").unwrap();
        add(&repo, &[]).unwrap();
        commit(&repo, "boot").unwrap();
        assert_eq!(current_branch(&repo).unwrap(), "main");
    }

    #[test]
    fn init_status_commit_log_roundtrip() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path().join("repo");

        init(&repo).unwrap();
        std::fs::write(repo.join("file.txt"), "hello\n").unwrap();
        add(&repo, &[]).unwrap();
        commit(&repo, "initial commit").unwrap();

        let log_output = log(&repo, 5).unwrap();
        assert!(log_output.contains("initial commit"));
        assert_eq!(current_branch(&repo).unwrap(), "main");
        assert!(!head_sha(&repo).unwrap().is_empty());
    }

    #[test]
    fn diff_and_branch_roundtrip() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path().join("repo");
        init(&repo).unwrap();
        std::fs::write(repo.join("file.txt"), "hello\n").unwrap();
        add(&repo, &[]).unwrap();
        commit(&repo, "initial").unwrap();

        std::fs::write(repo.join("file.txt"), "hello world\n").unwrap();
        let diff_text = diff(&repo).unwrap();
        assert!(diff_text.contains("hello world"));

        create_branch(&repo, "feat-x").unwrap();
        let names = branches(&repo).unwrap();
        assert!(names.iter().any(|n| n == "feat-x"));
        checkout(&repo, "feat-x").unwrap();
        assert_eq!(current_branch(&repo).unwrap(), "feat-x");
    }

    #[test]
    fn list_repos_finds_git_dirs() {
        let tmp = tempfile::tempdir().unwrap();
        let data = tmp.path();
        init(&data.join("alpha")).unwrap();
        init(&data.join("beta")).unwrap();
        std::fs::create_dir_all(data.join("not-git")).unwrap();
        let names = list_repos(data).unwrap();
        assert_eq!(names, vec!["alpha".to_string(), "beta".to_string()]);
    }
}
