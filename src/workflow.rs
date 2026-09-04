//! A minimal, local runner for GitHub Actions-like workflows, scoped per
//! repository.
//!
//! Workflow files live under `<repo>/.gitcell/workflows/*.yml` using a
//! deliberately small subset of the GitHub Actions schema:
//!
//! ```yaml
//! name: CI
//! on: [push, manual]
//! env:
//!   GREETING: hello
//! jobs:
//!   build:
//!     env:
//!       JOB_VAR: 1
//!     steps:
//!       - name: Say hello
//!         run: echo "$GREETING, $JOB_VAR"
//! ```
//!
//! Everything runs locally via the system shell on the server host -- there
//! is no containerization or remote execution, matching gitcell's
//! "local execution" philosophy even when served to multiple clients.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};

const WORKFLOWS_SUBDIR: &str = ".gitcell/workflows";

#[derive(Debug, Clone, Deserialize)]
struct WorkflowSpec {
    name: Option<String>,
    #[serde(rename = "on", default)]
    on: Option<serde_yaml::Value>,
    #[serde(default)]
    env: HashMap<String, serde_yaml::Value>,
    #[serde(default)]
    jobs: HashMap<String, JobSpec>,
}

#[derive(Debug, Clone, Deserialize)]
struct JobSpec {
    #[serde(default)]
    env: HashMap<String, serde_yaml::Value>,
    #[serde(default)]
    steps: Vec<StepSpec>,
}

#[derive(Debug, Clone, Deserialize)]
struct StepSpec {
    name: Option<String>,
    run: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct WorkflowSummary {
    pub name: String,
    pub file: String,
    pub events: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct StepResult {
    pub name: String,
    pub command: String,
    pub success: bool,
    pub stdout: String,
    pub stderr: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct JobResult {
    pub name: String,
    pub steps: Vec<StepResult>,
}

impl JobResult {
    pub fn ok(&self) -> bool {
        self.steps.iter().all(|s| s.success)
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct WorkflowResult {
    pub name: String,
    pub jobs: Vec<JobResult>,
}

impl WorkflowResult {
    pub fn ok(&self) -> bool {
        self.jobs.iter().all(|j| j.ok())
    }
}

fn normalize_events(raw_on: &Option<serde_yaml::Value>) -> Vec<String> {
    match raw_on {
        None => vec![],
        Some(serde_yaml::Value::String(s)) => vec![s.clone()],
        Some(serde_yaml::Value::Bool(_)) => vec![], // YAML 1.1 parses bare `on` as bool
        Some(serde_yaml::Value::Sequence(seq)) => seq
            .iter()
            .filter_map(|v| v.as_str().map(String::from))
            .collect(),
        Some(serde_yaml::Value::Mapping(map)) => map
            .keys()
            .filter_map(|k| k.as_str().map(String::from))
            .collect(),
        Some(_) => vec![],
    }
}

fn workflows_dir(repo_path: &Path) -> PathBuf {
    repo_path.join(WORKFLOWS_SUBDIR)
}

fn load(path: &Path) -> Result<WorkflowSpec> {
    let content = std::fs::read_to_string(path)
        .map_err(|e| Error::Workflow(format!("failed to read {}: {e}", path.display())))?;
    serde_yaml::from_str(&content)
        .map_err(|e| Error::Workflow(format!("invalid workflow {}: {e}", path.display())))
}

/// List all workflows defined in a repository.
pub fn discover(repo_path: &Path) -> Result<Vec<WorkflowSummary>> {
    let dir = workflows_dir(repo_path);
    if !dir.exists() {
        return Ok(vec![]);
    }

    let mut summaries = vec![];
    let mut entries: Vec<PathBuf> = std::fs::read_dir(&dir)
        .map_err(|e| Error::Workflow(format!("failed to read {}: {e}", dir.display())))?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| {
            matches!(
                p.extension().and_then(|e| e.to_str()),
                Some("yml") | Some("yaml")
            )
        })
        .collect();
    entries.sort();

    for path in entries {
        let spec = load(&path)?;
        let stem = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("workflow")
            .to_string();
        summaries.push(WorkflowSummary {
            name: spec.name.unwrap_or_else(|| stem.clone()),
            file: path
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or_default()
                .to_string(),
            events: normalize_events(&spec.on),
        });
    }
    Ok(summaries)
}

fn find_workflow_file(repo_path: &Path, name: &str) -> Result<PathBuf> {
    let dir = workflows_dir(repo_path);
    if !dir.exists() {
        return Err(Error::NotFound(format!(
            "no workflows directory in {}",
            repo_path.display()
        )));
    }
    for entry in std::fs::read_dir(&dir)
        .map_err(|e| Error::Workflow(format!("failed to read {}: {e}", dir.display())))?
    {
        let entry = entry.map_err(|e| Error::Workflow(e.to_string()))?;
        let path = entry.path();
        if !matches!(
            path.extension().and_then(|e| e.to_str()),
            Some("yml") | Some("yaml")
        ) {
            continue;
        }
        let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
        let spec = load(&path)?;
        let workflow_name = spec.name.unwrap_or_else(|| stem.to_string());
        if workflow_name == name || stem == name {
            return Ok(path);
        }
    }
    Err(Error::NotFound(format!("no workflow named {name:?} found")))
}

fn listens_for(events: &[String], event: &str) -> bool {
    if events.is_empty() {
        return true;
    }
    events.iter().any(|e| {
        e == event || (event == "commit" && e == "push") || (event == "push" && e == "commit")
    })
}

/// Run every workflow in the repo that listens for `event` (`commit` also
/// matches workflows declared as `on: [push]`).
pub fn run_for_event(repo_path: &Path, event: &str) -> Result<Vec<WorkflowResult>> {
    let summaries = discover(repo_path)?;
    let mut results = Vec::new();
    for summary in summaries {
        if listens_for(&summary.events, event) {
            results.push(run(repo_path, &summary.name, None)?);
        }
    }
    Ok(results)
}

/// Run a workflow by name, optionally restricted to a given trigger event.
pub fn run(repo_path: &Path, name: &str, event: Option<&str>) -> Result<WorkflowResult> {
    let path = find_workflow_file(repo_path, name)?;
    let spec = load(&path)?;
    let events = normalize_events(&spec.on);

    if let Some(event) = event
        && !listens_for(&events, event)
    {
        return Err(Error::Workflow(format!(
            "workflow {name:?} does not listen for event {event:?}"
        )));
    }

    let workflow_name = spec.name.clone().unwrap_or_else(|| name.to_string());
    let mut result = WorkflowResult {
        name: workflow_name,
        jobs: vec![],
    };

    for (job_name, job_spec) in &spec.jobs {
        let job_result = run_job(repo_path, job_name, job_spec, &spec.env);
        let ok = job_result.ok();
        result.jobs.push(job_result);
        if !ok {
            break;
        }
    }
    Ok(result)
}

fn run_job(
    repo_path: &Path,
    job_name: &str,
    job_spec: &JobSpec,
    workflow_env: &HashMap<String, serde_yaml::Value>,
) -> JobResult {
    let mut env: HashMap<String, String> = workflow_env
        .iter()
        .map(|(k, v)| (k.clone(), yaml_to_string(v)))
        .collect();
    for (k, v) in &job_spec.env {
        env.insert(k.clone(), yaml_to_string(v));
    }

    let mut job_result = JobResult {
        name: job_name.to_string(),
        steps: vec![],
    };
    for (index, step) in job_spec.steps.iter().enumerate() {
        let step_result = run_step(repo_path, index, step, &env);
        let ok = step_result.success;
        job_result.steps.push(step_result);
        if !ok {
            break;
        }
    }
    job_result
}

fn yaml_to_string(value: &serde_yaml::Value) -> String {
    match value {
        serde_yaml::Value::String(s) => s.clone(),
        other => serde_yaml::to_string(other)
            .unwrap_or_default()
            .trim()
            .to_string(),
    }
}

fn run_step(
    repo_path: &Path,
    index: usize,
    step: &StepSpec,
    env: &HashMap<String, String>,
) -> StepResult {
    let name = step.name.clone().unwrap_or_else(|| format!("step-{index}"));
    let output = Command::new("sh")
        .arg("-c")
        .arg(&step.run)
        .current_dir(repo_path)
        .envs(env)
        .output();

    match output {
        Ok(output) => StepResult {
            name,
            command: step.run.clone(),
            success: output.status.success(),
            stdout: String::from_utf8_lossy(&output.stdout).to_string(),
            stderr: String::from_utf8_lossy(&output.stderr).to_string(),
        },
        Err(e) => StepResult {
            name,
            command: step.run.clone(),
            success: false,
            stdout: String::new(),
            stderr: format!("failed to spawn step: {e}"),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn write_workflow(dir: &Path, content: &str) {
        let workflows = dir.join(WORKFLOWS_SUBDIR);
        fs::create_dir_all(&workflows).unwrap();
        fs::write(workflows.join("ci.yml"), content).unwrap();
    }

    #[test]
    fn discover_and_run_success() {
        let tmp = tempfile::tempdir().unwrap();
        write_workflow(
            tmp.path(),
            "name: CI\non: [push]\nenv:\n  GREETING: hi\njobs:\n  build:\n    steps:\n      - name: greet\n        run: echo \"$GREETING\"\n",
        );

        let workflows = discover(tmp.path()).unwrap();
        assert_eq!(workflows.len(), 1);
        assert_eq!(workflows[0].name, "CI");
        assert_eq!(workflows[0].events, vec!["push".to_string()]);

        let result = run(tmp.path(), "CI", Some("push")).unwrap();
        assert!(result.ok());
        assert_eq!(result.jobs[0].steps[0].stdout.trim(), "hi");
    }

    #[test]
    fn run_stops_on_failure() {
        let tmp = tempfile::tempdir().unwrap();
        write_workflow(
            tmp.path(),
            "name: CI\non: [push]\njobs:\n  build:\n    steps:\n      - name: fail\n        run: exit 1\n      - name: never-runs\n        run: echo should-not-print\n",
        );

        let result = run(tmp.path(), "CI", Some("push")).unwrap();
        assert!(!result.ok());
        assert_eq!(result.jobs[0].steps.len(), 1);
    }

    #[test]
    fn event_mismatch_errors() {
        let tmp = tempfile::tempdir().unwrap();
        write_workflow(
            tmp.path(),
            "name: CI\non: [push]\njobs:\n  build:\n    steps:\n      - name: noop\n        run: echo hi\n",
        );

        let err = run(tmp.path(), "CI", Some("pull_request")).unwrap_err();
        assert!(matches!(err, Error::Workflow(_)));
    }

    #[test]
    fn missing_workflow_errors() {
        let tmp = tempfile::tempdir().unwrap();
        let err = run(tmp.path(), "does-not-exist", None).unwrap_err();
        assert!(matches!(err, Error::NotFound(_)));
    }
}
