use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Server configuration.
///
/// Values can be overridden via environment variables so the same binary
/// can be deployed for multiple clients/environments without rebuilding:
/// - `GITCELL_HOST`
/// - `GITCELL_PORT`
/// - `GITCELL_DATABASE_URL`
/// - `GITCELL_DATA_DIR` (root directory holding all managed git repositories)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub host: String,
    pub port: u16,
    pub database_url: String,
    pub data_dir: PathBuf,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            host: std::env::var("GITCELL_HOST").unwrap_or_else(|_| "0.0.0.0".to_string()),
            port: std::env::var("GITCELL_PORT")
                .ok()
                .and_then(|p| p.parse().ok())
                .unwrap_or(8080),
            database_url: std::env::var("GITCELL_DATABASE_URL")
                .unwrap_or_else(|_| "sqlite://gitcell.db?mode=rwc".to_string()),
            data_dir: std::env::var("GITCELL_DATA_DIR")
                .map(PathBuf::from)
                .unwrap_or_else(|_| PathBuf::from("./data/repos")),
        }
    }
}
