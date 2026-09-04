use std::sync::Arc;

use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use cellz::cell::CellManager;
use cellz::storage::LocalBlobStore;
use gitcell::server::{self, AppState};
use http_body_util::BodyExt;
use tower::ServiceExt;

async fn test_state() -> (AppState, tempfile::TempDir) {
    let tmp = tempfile::tempdir().unwrap();
    let cells_dir = tmp.path().join("cells");
    let cells_storage_dir = tmp.path().join("cells-storage");
    let data_dir = tmp.path().join("repos");
    std::fs::create_dir_all(&cells_dir).unwrap();
    std::fs::create_dir_all(&cells_storage_dir).unwrap();
    std::fs::create_dir_all(&data_dir).unwrap();

    let blob_store = Arc::new(LocalBlobStore::new(&cells_storage_dir));
    let cell_manager = Arc::new(CellManager::new(&cells_dir, blob_store, 60));

    let state = AppState {
        cell_manager,
        data_dir,
    };
    (state, tmp)
}

#[tokio::test]
async fn test_health_check() {
    let (state, _tmp) = test_state().await;
    let app = server::create_router(state);

    let response = app
        .oneshot(
            Request::builder()
                .uri("/health")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

    assert_eq!(json["status"], "ok");
    assert_eq!(json["service"], "gitcell");
}

#[tokio::test]
async fn test_repo_init_and_status() {
    let (state, _tmp) = test_state().await;
    let app = server::create_router(state);

    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/repos/demo/init")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/repos/demo/status")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn test_repo_status_missing_repo_is_not_found() {
    let (state, _tmp) = test_state().await;
    let app = server::create_router(state);

    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/repos/does-not-exist/status")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn test_invalid_repo_name_is_rejected() {
    let (state, _tmp) = test_state().await;
    let app = server::create_router(state);

    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/repos/..%2f..%2fetc/status")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn test_prompts_record_and_list() {
    let (state, _tmp) = test_state().await;
    let app = server::create_router(state);

    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/repos/demo/prompts")
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::json!({"role": "user", "content": "hello"}).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/repos/demo/prompts")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json.as_array().unwrap().len(), 1);
    assert_eq!(json[0]["content"], "hello");
}

#[tokio::test]
async fn test_workflows_list_empty_when_missing() {
    let (state, _tmp) = test_state().await;
    let app = server::create_router(state);

    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/repos/demo/workflows")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json.as_array().unwrap().len(), 0);
}

async fn json_request(
    app: axum::Router,
    method: &str,
    uri: &str,
    body: Option<serde_json::Value>,
) -> (StatusCode, serde_json::Value) {
    let mut builder = Request::builder().method(method).uri(uri);
    let body = if let Some(payload) = body {
        builder = builder.header("content-type", "application/json");
        Body::from(payload.to_string())
    } else {
        Body::empty()
    };
    let response = app.oneshot(builder.body(body).unwrap()).await.unwrap();
    let status = response.status();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    let json = if bytes.is_empty() {
        serde_json::Value::Null
    } else {
        serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null)
    };
    (status, json)
}

#[tokio::test]
async fn personal_loop_init_commit_workflow_prompt_log() {
    let (state, tmp) = test_state().await;
    let data_dir = state.data_dir.clone();
    let app = server::create_router(state);

    let (status, _) = json_request(app.clone(), "POST", "/api/v1/repos/demo/init", None).await;
    assert_eq!(status, StatusCode::OK);

    let repo = data_dir.join("demo");
    std::fs::write(repo.join("README.md"), "hello gitcell\n").unwrap();
    let wf_dir = repo.join(".gitcell/workflows");
    std::fs::create_dir_all(&wf_dir).unwrap();
    std::fs::write(
        wf_dir.join("ci.yml"),
        "name: CI\non: [push]\njobs:\n  build:\n    steps:\n      - name: ok\n        run: echo ci-ok\n",
    )
    .unwrap();

    let (status, commit) = json_request(
        app.clone(),
        "POST",
        "/api/v1/repos/demo/commit",
        Some(serde_json::json!({"message": "initial commit"})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "commit failed: {commit}");
    let sha = commit["sha"].as_str().unwrap().to_string();
    assert!(!sha.is_empty());
    assert_eq!(commit["branch"], "main");
    assert_eq!(commit["workflows"][0]["name"], "CI");
    assert_eq!(
        commit["workflows"][0]["jobs"][0]["steps"][0]["success"],
        true
    );

    let (status, prompt) = json_request(
        app.clone(),
        "POST",
        "/api/v1/repos/demo/prompts",
        Some(serde_json::json!({
            "role": "user",
            "content": "add a readme"
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(prompt["content"], "add a readme");
    assert_eq!(prompt["commit"], sha);
    assert_eq!(prompt["branch"], "main");

    let (status, listed) = json_request(
        app.clone(),
        "GET",
        &format!("/api/v1/repos/demo/prompts?commit={sha}"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(listed.as_array().unwrap().len(), 1);

    let (status, log) = json_request(app.clone(), "GET", "/api/v1/repos/demo/log", None).await;
    assert_eq!(status, StatusCode::OK);
    assert!(log["log"].as_str().unwrap().contains("initial commit"));

    let (status, repos) = json_request(app.clone(), "GET", "/api/v1/repos", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(repos["repos"], serde_json::json!(["demo"]));

    let (status, _) = json_request(
        app.clone(),
        "POST",
        "/api/v1/repos/demo/branches",
        Some(serde_json::json!({"name": "feat-x", "checkout": true})),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (status, branches) =
        json_request(app.clone(), "GET", "/api/v1/repos/demo/branches", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(branches["current"], "feat-x");

    let _ = tmp;
}
