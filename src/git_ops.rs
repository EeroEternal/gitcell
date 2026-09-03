//! Thin wrappers around common git operations, scoped by repository name so
//! a single gitcell server instance can host many repositories for many
//! clients.
//!
//! gitcell does not reimplement git; it shells out to the system `git`
//! binary underneath each managed repository's working directory.

use std::path::{Path, PathBuf};
use std::process::Command;

use crate::error::{Error, Result};

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
        Err(Error::Git(result.stderr))
    }
}

/// Initialize a new git repository at `path`, creating the directory if
/// needed.
pub fn init(path: &Path) -> Result<GitOutput> {
    std::fs::create_dir_all(path)
        .map_err(|e| Error::Git(format!("failed to create repo directory: {e}")))?;
    require_ok(run(path, &["init"])?)
}

pub fn status(path: &Path) -> Result<String> {
    Ok(require_ok(run(path, &["status", "--short", "--branch"])?)?.stdout)
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

    fn configure_identity(path: &Path) {
        run(path, &["config", "user.email", "test@example.com"]).unwrap();
        run(path, &["config", "user.name", "Test"]).unwrap();
    }

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
    fn init_status_commit_log_roundtrip() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path().join("repo");

        init(&repo).unwrap();
        assert!(is_git_repo(&repo));
        configure_identity(&repo);

        std::fs::write(repo.join("file.txt"), "hello\n").unwrap();
        add(&repo, &[]).unwrap();
        commit(&repo, "initial commit").unwrap();

        let log_output = log(&repo, 5).unwrap();
        assert!(log_output.contains("initial commit"));
    }
}
