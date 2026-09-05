use std::sync::Arc;

use axum::{
    Json, Router,
    extract::{Path as AxumPath, Query, State},
    routing::{get, post},
};
use cellz::cell::CellManager;
use cellz::storage::LocalBlobStore;
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

impl AppState {
    pub fn new(
        data_dir: impl AsRef<std::path::Path>,
        cells_dir: impl AsRef<std::path::Path>,
        cells_storage_dir: impl AsRef<std::path::Path>,
        lease_ttl_secs: u64,
    ) -> anyhow::Result<Self> {
        let data_dir = data_dir.as_ref().to_path_buf();
        let cells_dir = cells_dir.as_ref().to_path_buf();
        let cells_storage_dir = cells_storage_dir.as_ref().to_path_buf();
        std::fs::create_dir_all(&data_dir)?;
        std::fs::create_dir_all(&cells_dir)?;
        std::fs::create_dir_all(&cells_storage_dir)?;
        let blob_store = Arc::new(LocalBlobStore::new(&cells_storage_dir));
        let cell_manager = Arc::new(CellManager::new(&cells_dir, blob_store, lease_ttl_secs));
        Ok(Self {
            cell_manager,
            data_dir,
        })
    }
}

/// Repo / prompt / workflow API without `/health`. For embedding (e.g. OpenHub).
pub fn api_router() -> Router<AppState> {
    Router::new()
        .route("/api/v1/repos", get(repos_list))
        .route("/api/v1/repos/{repo}/init", post(repo_init))
        .route("/api/v1/repos/{repo}/status", get(repo_status))
        .route("/api/v1/repos/{repo}/diff", get(repo_diff))
        .route("/api/v1/repos/{repo}/commit", post(repo_commit))
        .route("/api/v1/repos/{repo}/log", get(repo_log))
        .route(
            "/api/v1/repos/{repo}/branches",
            get(repo_branches).post(repo_create_branch),
        )
        .route("/api/v1/repos/{repo}/checkout", post(repo_checkout))
        .route("/api/v1/repos/{repo}/show/{rev}", get(repo_show))
        .route("/api/v1/repos/{repo}/tree", get(repo_tree))
        .route(
            "/api/v1/repos/{repo}/files/{*path}",
            get(repo_get_file).put(repo_put_file),
        )
        .route(
            "/api/v1/repos/{repo}/prompts",
            get(prompts_list).post(prompts_record),
        )
        .route("/api/v1/repos/{repo}/workflows", get(workflows_list))
        .route(
            "/api/v1/repos/{repo}/workflows/{name}/run",
            post(workflow_run),
        )
}

pub fn create_router(state: AppState) -> Router {
    Router::new()
        .route("/health", get(health_check))
        .route("/api/v1/ping", get(ping))
        .merge(api_router())
        .layer(TraceLayer::new_for_http())
        .layer(CorsLayer::permissive())
        .with_state(state)
}

fn require_repo(data_dir: &std::path::Path, repo: &str) -> Result<std::path::PathBuf> {
    let path = git_ops::repo_path(data_dir, repo)?;
    if !git_ops::is_git_repo(&path) {
        return Err(Error::NotFound(format!("repository {repo:?} not found")));
    }
    Ok(path)
}

fn head_context(path: &std::path::Path) -> (Option<String>, Option<String>) {
    (
        git_ops::head_sha(path).ok(),
        git_ops::current_branch(path).ok(),
    )
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

async fn repos_list(State(state): State<AppState>) -> Result<Json<Value>> {
    let repos = git_ops::list_repos(&state.data_dir)?;
    Ok(Json(json!({ "repos": repos })))
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
    let path = require_repo(&state.data_dir, &repo)?;
    let status = git_ops::status(&path)?;
    Ok(Json(json!({ "repo": repo, "status": status })))
}

async fn repo_diff(
    State(state): State<AppState>,
    AxumPath(repo): AxumPath<String>,
) -> Result<Json<Value>> {
    let path = require_repo(&state.data_dir, &repo)?;
    let diff = git_ops::diff(&path)?;
    let status = git_ops::status(&path)?;
    Ok(Json(
        json!({ "repo": repo, "diff": diff, "status": status }),
    ))
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
    let path = require_repo(&state.data_dir, &repo)?;
    if payload.message.trim().is_empty() {
        return Err(Error::InvalidRequest(
            "commit message must not be empty".into(),
        ));
    }
    git_ops::add(&path, &payload.paths)?;
    let result = git_ops::commit(&path, &payload.message)?;
    let sha = git_ops::head_sha(&path).ok();
    let branch = git_ops::current_branch(&path).ok();

    if let (Some(sha), Some(branch)) = (sha.as_deref(), branch.as_deref()) {
        let _ = storage::record_commit(
            &state.cell_manager,
            &repo,
            sha,
            payload.message.trim(),
            branch,
        )
        .await;
    }

    let workflows = workflow::run_for_event(&path, "commit").unwrap_or_default();
    for wf in &workflows {
        let _ = storage::record_workflow(&state.cell_manager, &repo, sha.as_deref(), wf).await;
    }

    Ok(Json(json!({
        "repo": repo,
        "sha": sha,
        "branch": branch,
        "output": result.stdout,
        "workflows": workflows,
    })))
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
    let path = require_repo(&state.data_dir, &repo)?;
    let log = git_ops::log(&path, query.limit.unwrap_or(10))?;
    Ok(Json(json!({ "repo": repo, "log": log })))
}

async fn repo_branches(
    State(state): State<AppState>,
    AxumPath(repo): AxumPath<String>,
) -> Result<Json<Value>> {
    let path = require_repo(&state.data_dir, &repo)?;
    let branches = git_ops::branches(&path)?;
    let current = git_ops::current_branch(&path).ok();
    Ok(Json(
        json!({ "repo": repo, "branches": branches, "current": current }),
    ))
}

#[derive(Debug, Deserialize)]
struct BranchRequest {
    name: String,
    #[serde(default)]
    checkout: bool,
}

async fn repo_create_branch(
    State(state): State<AppState>,
    AxumPath(repo): AxumPath<String>,
    Json(payload): Json<BranchRequest>,
) -> Result<Json<Value>> {
    let path = require_repo(&state.data_dir, &repo)?;
    git_ops::create_branch(&path, &payload.name)?;
    if payload.checkout {
        git_ops::checkout(&path, &payload.name)?;
    }
    Ok(Json(json!({
        "repo": repo,
        "branch": payload.name,
        "current": git_ops::current_branch(&path).ok(),
    })))
}

#[derive(Debug, Deserialize)]
struct CheckoutRequest {
    name: String,
}

async fn repo_checkout(
    State(state): State<AppState>,
    AxumPath(repo): AxumPath<String>,
    Json(payload): Json<CheckoutRequest>,
) -> Result<Json<Value>> {
    let path = require_repo(&state.data_dir, &repo)?;
    git_ops::checkout(&path, &payload.name)?;
    Ok(Json(json!({
        "repo": repo,
        "current": git_ops::current_branch(&path).ok(),
    })))
}

async fn repo_show(
    State(state): State<AppState>,
    AxumPath((repo, rev)): AxumPath<(String, String)>,
) -> Result<Json<Value>> {
    let path = require_repo(&state.data_dir, &repo)?;
    let show = git_ops::show(&path, &rev)?;
    Ok(Json(json!({ "repo": repo, "rev": rev, "show": show })))
}

async fn repo_tree(
    State(state): State<AppState>,
    AxumPath(repo): AxumPath<String>,
) -> Result<Json<Value>> {
    let path = require_repo(&state.data_dir, &repo)?;
    let tree = git_ops::list_tree(&path)?;
    Ok(Json(json!({ "repo": repo, "tree": tree })))
}

async fn repo_get_file(
    State(state): State<AppState>,
    AxumPath((repo, file_path)): AxumPath<(String, String)>,
) -> Result<Json<Value>> {
    let path = require_repo(&state.data_dir, &repo)?;
    let content = git_ops::read_file(&path, &file_path)?;
    Ok(Json(
        json!({ "repo": repo, "path": file_path, "content": content }),
    ))
}

#[derive(Debug, Deserialize)]
struct FilePutRequest {
    content: String,
}

async fn repo_put_file(
    State(state): State<AppState>,
    AxumPath((repo, file_path)): AxumPath<(String, String)>,
    Json(payload): Json<FilePutRequest>,
) -> Result<Json<Value>> {
    let path = require_repo(&state.data_dir, &repo)?;
    git_ops::write_file(&path, &file_path, &payload.content)?;
    Ok(Json(
        json!({ "repo": repo, "path": file_path, "written": true }),
    ))
}

async fn prompts_record(
    State(state): State<AppState>,
    AxumPath(repo): AxumPath<String>,
    Json(mut payload): Json<NewInteraction>,
) -> Result<Json<Value>> {
    git_ops::validate_repo_name(&repo)?;
    if payload.role.trim().is_empty() || payload.content.trim().is_empty() {
        return Err(Error::InvalidRequest(
            "role and content must not be empty".into(),
        ));
    }
    if let Ok(path) = require_repo(&state.data_dir, &repo) {
        let (sha, branch) = head_context(&path);
        if payload.commit.is_none() {
            payload.commit = sha;
        }
        if payload.branch.is_none() {
            payload.branch = branch;
        }
    }
    let interaction = storage::record(&state.cell_manager, &repo, payload).await?;
    Ok(Json(json!(interaction)))
}

#[derive(Debug, Deserialize)]
struct PromptsListQuery {
    role: Option<String>,
    commit: Option<String>,
    since: Option<i64>,
    limit: Option<i64>,
}

async fn prompts_list(
    State(state): State<AppState>,
    AxumPath(repo): AxumPath<String>,
    Query(query): Query<PromptsListQuery>,
) -> Result<Json<Value>> {
    git_ops::validate_repo_name(&repo)?;
    let commit_filter = match query.commit.as_deref() {
        Some("HEAD") => require_repo(&state.data_dir, &repo)
            .ok()
            .and_then(|path| git_ops::head_sha(&path).ok()),
        other => other.map(str::to_string),
    };
    let interactions = storage::list(
        &state.cell_manager,
        &repo,
        query.role.as_deref(),
        commit_filter.as_deref(),
        query.since,
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
