//! Agent prompt history and git/workflow events, backed by embedded cellz
//! (`default-features = false`). One cell per repository, keyed by repo name.

use cellz::cell::CellManager;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::error::{Error, Result};
use crate::workflow::WorkflowResult;

const PROMPT_EVENT_TYPE: &str = "gitcell.prompt";
const COMMIT_EVENT_TYPE: &str = "gitcell.commit";
const WORKFLOW_EVENT_TYPE: &str = "gitcell.workflow";

/// Cap mixed-event fetches so we never scan 100k rows in memory.
const MAX_FETCH: i64 = 1000;
const STDOUT_CAP: usize = 4096;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Interaction {
    pub id: String,
    pub repo: String,
    pub role: String,
    pub content: String,
    pub metadata: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub commit: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub branch: Option<String>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct NewInteraction {
    pub role: String,
    pub content: String,
    #[serde(default)]
    pub metadata: Option<serde_json::Value>,
    #[serde(default)]
    pub commit: Option<String>,
    #[serde(default)]
    pub branch: Option<String>,
}

/// Append a prompt/response interaction to the repository's cellz cell.
pub async fn record(
    manager: &CellManager,
    repo: &str,
    payload: NewInteraction,
) -> Result<Interaction> {
    let handle = manager.get_or_activate(repo).await?;

    let event_payload = json!({
        "role": payload.role,
        "content": payload.content,
        "metadata": payload.metadata,
        "commit": payload.commit,
        "branch": payload.branch,
    });

    let event = handle
        .append_event(None, PROMPT_EVENT_TYPE, event_payload)
        .await?;

    event_to_interaction(repo, event)
}

pub async fn record_commit(
    manager: &CellManager,
    repo: &str,
    sha: &str,
    message: &str,
    branch: &str,
) -> Result<()> {
    let handle = manager.get_or_activate(repo).await?;
    handle
        .append_event(
            None,
            COMMIT_EVENT_TYPE,
            json!({
                "sha": sha,
                "message": message,
                "branch": branch,
            }),
        )
        .await?;
    Ok(())
}

pub async fn record_workflow(
    manager: &CellManager,
    repo: &str,
    sha: Option<&str>,
    result: &WorkflowResult,
) -> Result<()> {
    let handle = manager.get_or_activate(repo).await?;
    let jobs: Vec<serde_json::Value> = result
        .jobs
        .iter()
        .map(|job| {
            json!({
                "name": job.name,
                "ok": job.ok(),
                "steps": job.steps.iter().map(|step| {
                    json!({
                        "name": step.name,
                        "command": step.command,
                        "success": step.success,
                        "stdout": truncate(&step.stdout),
                        "stderr": truncate(&step.stderr),
                    })
                }).collect::<Vec<_>>(),
            })
        })
        .collect();

    handle
        .append_event(
            None,
            WORKFLOW_EVENT_TYPE,
            json!({
                "name": result.name,
                "ok": result.ok(),
                "commit": sha,
                "jobs": jobs,
            }),
        )
        .await?;
    Ok(())
}

/// List prompt interactions, optionally filtered by role and commit SHA.
/// `since` is a cellz event sequence (exclusive), same as cellz `get_events`.
pub async fn list(
    manager: &CellManager,
    repo: &str,
    role: Option<&str>,
    commit: Option<&str>,
    since: Option<i64>,
    limit: i64,
) -> Result<Vec<Interaction>> {
    let handle = manager.get_or_activate(repo).await?;
    let limit = limit.max(0);
    let fetch = (limit.saturating_mul(8)).clamp(1, MAX_FETCH);

    let events = handle.get_events(since, Some(fetch)).await?;

    let mut interactions = events
        .into_iter()
        .filter(|event| event.event_type == PROMPT_EVENT_TYPE)
        .filter_map(|event| event_to_interaction(repo, event).ok())
        .filter(|interaction| role.is_none_or(|r| interaction.role == r))
        .filter(|interaction| {
            commit.is_none_or(|c| {
                interaction
                    .commit
                    .as_deref()
                    .is_some_and(|sha| sha.starts_with(c) || c.starts_with(sha))
            })
        })
        .collect::<Vec<_>>();

    let keep = limit as usize;
    if interactions.len() > keep {
        let skip = interactions.len() - keep;
        interactions = interactions.split_off(skip);
    }

    Ok(interactions)
}

fn truncate(s: &str) -> String {
    if s.len() <= STDOUT_CAP {
        s.to_string()
    } else {
        let mut cut = STDOUT_CAP;
        while cut > 0 && !s.is_char_boundary(cut) {
            cut -= 1;
        }
        format!("{}…", &s[..cut])
    }
}

fn event_to_interaction(
    repo: &str,
    event: cellz::model::event::EventRecord,
) -> Result<Interaction> {
    let role = event
        .payload
        .get("role")
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_string();
    let content = event
        .payload
        .get("content")
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_string();
    let metadata = event
        .payload
        .get("metadata")
        .cloned()
        .filter(|v| !v.is_null());
    let commit = event
        .payload
        .get("commit")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .map(str::to_string);
    let branch = event
        .payload
        .get("branch")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .map(str::to_string);

    if role.is_empty() {
        return Err(Error::Internal(anyhow::anyhow!(
            "malformed gitcell.prompt event {} in cell {repo:?}: missing role",
            event.id
        )));
    }

    Ok(Interaction {
        id: event.id,
        repo: repo.to_string(),
        role,
        content,
        metadata,
        commit,
        branch,
        created_at: event.created_at,
    })
}
