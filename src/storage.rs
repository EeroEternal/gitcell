//! Central storage for agent/prompt interaction history.
//!
//! Every prompt exchanged with an agent (human prompt, agent response,
//! tool call, etc.) is recorded in a single SQLite database shared by all
//! repositories hosted by this gitcell server, keyed by repository name so
//! multiple clients/repositories can be served from one instance.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::{Row, SqlitePool};

use crate::error::Result;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Interaction {
    pub id: i64,
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

/// Run pending migrations against the given pool.
pub async fn migrate(pool: &SqlitePool) -> Result<()> {
    sqlx::migrate!("./migrations").run(pool).await?;
    Ok(())
}

pub async fn record(pool: &SqlitePool, repo: &str, payload: NewInteraction) -> Result<Interaction> {
    let metadata_json = payload
        .metadata
        .as_ref()
        .map(serde_json::to_string)
        .transpose()
        .map_err(|e| crate::error::Error::InvalidRequest(format!("invalid metadata: {e}")))?;

    let row = sqlx::query(
        "INSERT INTO interactions (repo, role, content, metadata, created_at) \
         VALUES (?, ?, ?, ?, ?) \
         RETURNING id, repo, role, content, metadata, created_at",
    )
    .bind(repo)
    .bind(&payload.role)
    .bind(&payload.content)
    .bind(&metadata_json)
    .bind(Utc::now())
    .fetch_one(pool)
    .await?;

    row_to_interaction(row)
}

pub async fn list(
    pool: &SqlitePool,
    repo: &str,
    role: Option<&str>,
    limit: i64,
) -> Result<Vec<Interaction>> {
    let rows = if let Some(role) = role {
        sqlx::query(
            "SELECT id, repo, role, content, metadata, created_at FROM interactions \
             WHERE repo = ? AND role = ? ORDER BY id DESC LIMIT ?",
        )
        .bind(repo)
        .bind(role)
        .bind(limit)
        .fetch_all(pool)
        .await?
    } else {
        sqlx::query(
            "SELECT id, repo, role, content, metadata, created_at FROM interactions \
             WHERE repo = ? ORDER BY id DESC LIMIT ?",
        )
        .bind(repo)
        .bind(limit)
        .fetch_all(pool)
        .await?
    };

    let mut interactions = rows
        .into_iter()
        .map(row_to_interaction)
        .collect::<Result<Vec<_>>>()?;
    interactions.reverse();
    Ok(interactions)
}

fn row_to_interaction(row: sqlx::sqlite::SqliteRow) -> Result<Interaction> {
    let metadata_str: Option<String> = row.try_get("metadata")?;
    let metadata = metadata_str
        .map(|s| serde_json::from_str(&s))
        .transpose()
        .map_err(|e| crate::error::Error::Internal(anyhow::anyhow!(e)))?;

    Ok(Interaction {
        id: row.try_get("id")?,
        repo: row.try_get("repo")?,
        role: row.try_get("role")?,
        content: row.try_get("content")?,
        metadata,
        created_at: row.try_get("created_at")?,
    })
}
