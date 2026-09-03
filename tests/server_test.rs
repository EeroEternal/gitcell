use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use gitcell::server::{self, AppState};
use http_body_util::BodyExt;
use sqlx::sqlite::SqlitePoolOptions;
use tower::ServiceExt;

async fn test_state() -> (AppState, tempfile::TempDir) {
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .unwrap();
    gitcell::storage::migrate(&pool).await.unwrap();

    let tmp = tempfile::tempdir().unwrap();
    let state = AppState {
        pool,
        data_dir: tmp.path().to_path_buf(),
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
