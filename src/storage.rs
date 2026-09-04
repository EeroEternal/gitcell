//! Central storage for agent/prompt interaction history, backed by the
//! embedded [`cellz`](https://crates.io/crates/cellz) event-sourced cell
//! engine (`cellz = { version = "0.2", default-features = false }`).
//!
//! Every prompt exchanged with an agent (human prompt, agent response,
//! tool call, etc.) for a given repository is appended as a durable
//! `gitcell.prompt` event to that repository's `cellz` cell -- one cell per
//! repository, keyed by repository name -- so multiple clients/repositories
//! can be served from a single gitcell instance while getting a replayable
//! event log, KV state, and checkpoints for free from `cellz`.

use cellz::cell::CellManager;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::error::{Error, Result};

/// `cellz` event type used to record gitcell agent/prompt interactions.
const PROMPT_EVENT_TYPE: &str = "gitcell.prompt";

/// Number of past events fetched from a cell before filtering/limiting.
const EVENT_FETCH_LIMIT: i64 = 100_000;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Interaction {
    pub id: String,
    pub repo: String,
    pub role: String,
    pub content: String,
    pub metadata: Option<serde_json::Value>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct NewInteraction {
    pub role: String,
    pub content: String,
    #[serde(default)]
    pub metadata: Option<serde_json::Value>,
}

/// Append a prompt/response interaction to the repository's `cellz` cell.
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
    });

    let event = handle
        .append_event(None, PROMPT_EVENT_TYPE, event_payload)
        .await?;

    event_to_interaction(repo, event)
}

/// List recorded interactions for a repository, optionally filtered by
/// role, most recent `limit` entries in chronological order.
pub async fn list(
    manager: &CellManager,
    repo: &str,
    role: Option<&str>,
    limit: i64,
) -> Result<Vec<Interaction>> {
    let handle = manager.get_or_activate(repo).await?;

    let events = handle.get_events(None, Some(EVENT_FETCH_LIMIT)).await?;

    let mut interactions = events
        .into_iter()
        .filter(|event| event.event_type == PROMPT_EVENT_TYPE)
        .filter_map(|event| event_to_interaction(repo, event).ok())
        .filter(|interaction| role.is_none_or(|r| interaction.role == r))
        .collect::<Vec<_>>();

    let limit = limit.max(0) as usize;
    if interactions.len() > limit {
        let skip = interactions.len() - limit;
        interactions = interactions.split_off(skip);
    }

    Ok(interactions)
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
        created_at: event.created_at,
    })
}
