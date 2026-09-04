use std::sync::Arc;

use axum::{
    Json, Router,
    extract::{Path as AxumPath, Query, State},
    routing::{get, post},
};
use cellz::cell::CellManager;
use serde::Deserialize;
use serde_json::{Value, json};
use tower_http::cors::CorsLayer;
use tower_http::trace::TraceLayer;

use crate::error::{Error, Result};
use crate::storage::{self, NewInteraction};
use crate::{git_ops, workflow};

#[derive(Clone)]
pub struct AppState {
    pub cell_manager: Arc<CellManager>,
    pub data_dir: std::path::PathBuf,
}

pub fn create_router(state: AppState) -> Router {
    Router::new()
        .route("/health", get(health_check))
        .route("/api/v1/ping", get(ping))
        .route("/api/v1/repos/{repo}/init", post(repo_init))
        .route("/api/v1/repos/{repo}/status", get(repo_status))
        .route("/api/v1/repos/{repo}/commit", post(repo_commit))
        .route("/api/v1/repos/{repo}/log", get(repo_log))
        .route(
            "/api/v1/repos/{repo}/prompts",
            get(prompts_list).post(prompts_record),
        )
        .route("/api/v1/repos/{repo}/workflows", get(workflows_list))
        .route(
            "/api/v1/repos/{repo}/workflows/{name}/run",
            post(workflow_run),
        )
        .layer(TraceLayer::new_for_http())
        .layer(CorsLayer::permissive())
        .with_state(state)
}

async fn health_check() -> Json<Value> {
    Json(json!({
        "status": "ok",
        "service": "gitcell"
    }))
}

async fn ping() -> Json<Value> {
    Json(json!({
        "message": "pong"
    }))
}

async fn repo_init(
    State(state): State<AppState>,
    AxumPath(repo): AxumPath<String>,
) -> Result<Json<Value>> {
    let path = git_ops::repo_path(&state.data_dir, &repo)?;
    git_ops::init(&path)?;
    Ok(Json(json!({ "repo": repo, "initialized": true })))
}

async fn repo_status(
    State(state): State<AppState>,
    AxumPath(repo): AxumPath<String>,
) -> Result<Json<Value>> {
    let path = git_ops::repo_path(&state.data_dir, &repo)?;
    if !git_ops::is_git_repo(&path) {
        return Err(Error::NotFound(format!("repository {repo:?} not found")));
    }
    let status = git_ops::status(&path)?;
    Ok(Json(json!({ "repo": repo, "status": status })))
}

#[derive(Debug, Deserialize)]
struct CommitRequest {
    message: String,
    #[serde(default)]
    paths: Vec<String>,
}

async fn repo_commit(
    State(state): State<AppState>,
    AxumPath(repo): AxumPath<String>,
    Json(payload): Json<CommitRequest>,
) -> Result<Json<Value>> {
    let path = git_ops::repo_path(&state.data_dir, &repo)?;
    if !git_ops::is_git_repo(&path) {
        return Err(Error::NotFound(format!("repository {repo:?} not found")));
    }
    if payload.message.trim().is_empty() {
        return Err(Error::InvalidRequest(
            "commit message must not be empty".into(),
        ));
    }
    git_ops::add(&path, &payload.paths)?;
    let result = git_ops::commit(&path, &payload.message)?;
    Ok(Json(json!({ "repo": repo, "output": result.stdout })))
}

#[derive(Debug, Deserialize)]
struct LogQuery {
    limit: Option<u32>,
}

async fn repo_log(
    State(state): State<AppState>,
    AxumPath(repo): AxumPath<String>,
    Query(query): Query<LogQuery>,
) -> Result<Json<Value>> {
    let path = git_ops::repo_path(&state.data_dir, &repo)?;
    if !git_ops::is_git_repo(&path) {
        return Err(Error::NotFound(format!("repository {repo:?} not found")));
    }
    let log = git_ops::log(&path, query.limit.unwrap_or(10))?;
    Ok(Json(json!({ "repo": repo, "log": log })))
}

async fn prompts_record(
    State(state): State<AppState>,
    AxumPath(repo): AxumPath<String>,
    Json(payload): Json<NewInteraction>,
) -> Result<Json<Value>> {
    git_ops::validate_repo_name(&repo)?;
    if payload.role.trim().is_empty() || payload.content.trim().is_empty() {
        return Err(Error::InvalidRequest(
            "role and content must not be empty".into(),
        ));
    }
    let interaction = storage::record(&state.cell_manager, &repo, payload).await?;
    Ok(Json(json!(interaction)))
}

#[derive(Debug, Deserialize)]
struct PromptsListQuery {
    role: Option<String>,
    limit: Option<i64>,
}

async fn prompts_list(
    State(state): State<AppState>,
    AxumPath(repo): AxumPath<String>,
    Query(query): Query<PromptsListQuery>,
) -> Result<Json<Value>> {
    git_ops::validate_repo_name(&repo)?;
    let interactions = storage::list(
        &state.cell_manager,
        &repo,
        query.role.as_deref(),
        query.limit.unwrap_or(20),
    )
    .await?;
    Ok(Json(json!(interactions)))
}

async fn workflows_list(
    State(state): State<AppState>,
    AxumPath(repo): AxumPath<String>,
) -> Result<Json<Value>> {
    let path = git_ops::repo_path(&state.data_dir, &repo)?;
    let workflows = workflow::discover(&path)?;
    Ok(Json(json!(workflows)))
}

#[derive(Debug, Deserialize)]
struct WorkflowRunQuery {
    event: Option<String>,
}

async fn workflow_run(
    State(state): State<AppState>,
    AxumPath((repo, name)): AxumPath<(String, String)>,
    Query(query): Query<WorkflowRunQuery>,
) -> Result<Json<Value>> {
    let path = git_ops::repo_path(&state.data_dir, &repo)?;
    let result = workflow::run(&path, &name, query.event.as_deref())?;
    Ok(Json(json!(result)))
}
