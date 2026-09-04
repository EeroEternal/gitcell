use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Server configuration.
///
/// Values can be overridden via environment variables so the same binary
/// can be deployed for multiple clients/environments without rebuilding:
/// - `GITCELL_HOST`
/// - `GITCELL_PORT`
/// - `GITCELL_DATA_DIR` (root directory holding all managed git repositories)
/// - `GITCELL_CELLS_DIR` (root directory for the `cellz` per-repo event
///   logs backing agent/prompt interaction history)
/// - `GITCELL_CELLS_STORAGE_DIR` (blob storage used by `cellz` for cell
///   snapshots/backups)
/// - `GITCELL_CELLS_LEASE_TTL_SECS` (single-writer lease TTL for `cellz`)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub host: String,
    pub port: u16,
    pub data_dir: PathBuf,
    pub cells_dir: PathBuf,
    pub cells_storage_dir: PathBuf,
    pub cells_lease_ttl_secs: u64,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            host: std::env::var("GITCELL_HOST").unwrap_or_else(|_| "0.0.0.0".to_string()),
            port: std::env::var("GITCELL_PORT")
                .ok()
                .and_then(|p| p.parse().ok())
                .unwrap_or(8080),
            data_dir: std::env::var("GITCELL_DATA_DIR")
                .map(PathBuf::from)
                .unwrap_or_else(|_| PathBuf::from("./data/repos")),
            cells_dir: std::env::var("GITCELL_CELLS_DIR")
                .map(PathBuf::from)
                .unwrap_or_else(|_| PathBuf::from("./data/cells")),
            cells_storage_dir: std::env::var("GITCELL_CELLS_STORAGE_DIR")
                .map(PathBuf::from)
                .unwrap_or_else(|_| PathBuf::from("./data/cells-storage")),
            cells_lease_ttl_secs: std::env::var("GITCELL_CELLS_LEASE_TTL_SECS")
                .ok()
                .and_then(|p| p.parse().ok())
                .unwrap_or(60),
        }
    }
}
