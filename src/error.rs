use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde_json::json;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Database error: {0}")]
    Database(#[from] sqlx::Error),

    #[error("Configuration error: {0}")]
    Config(String),

    #[error("Not found: {0}")]
    NotFound(String),

    #[error("Invalid request: {0}")]
    InvalidRequest(String),

    #[error("Git error: {0}")]
    Git(String),

    #[error("Workflow error: {0}")]
    Workflow(String),

    #[error("Internal error: {0}")]
    Internal(#[from] anyhow::Error),

    #[error("Migration error: {0}")]
    Migration(#[from] sqlx::migrate::MigrateError),
}

impl IntoResponse for Error {
    fn into_response(self) -> Response {
        let (status, message) = match &self {
            Error::NotFound(msg) => (StatusCode::NOT_FOUND, msg.clone()),
            Error::InvalidRequest(msg) => (StatusCode::BAD_REQUEST, msg.clone()),
            Error::Git(msg) => (StatusCode::UNPROCESSABLE_ENTITY, msg.clone()),
            Error::Workflow(msg) => (StatusCode::UNPROCESSABLE_ENTITY, msg.clone()),
            Error::Database(err) => (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()),
            Error::Config(msg) => (StatusCode::BAD_REQUEST, msg.clone()),
            Error::Internal(err) => (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()),
            Error::Migration(err) => (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()),
        };

        let body = Json(json!({
            "error": {
                "message": message,
                "code": status.as_u16(),
            }
        }));

        (status, body).into_response()
    }
}
