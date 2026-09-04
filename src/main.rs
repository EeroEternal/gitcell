use std::sync::Arc;

use cellz::cell::CellManager;
use cellz::storage::LocalBlobStore;
use gitcell::{config::Config, error::Result, server};
use tracing::info;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    let config = Config::default();

    std::fs::create_dir_all(&config.data_dir)
        .map_err(|e| anyhow::anyhow!("Failed to create data dir {:?}: {}", config.data_dir, e))?;
    std::fs::create_dir_all(&config.cells_dir)
        .map_err(|e| anyhow::anyhow!("Failed to create cells dir {:?}: {}", config.cells_dir, e))?;
    std::fs::create_dir_all(&config.cells_storage_dir).map_err(|e| {
        anyhow::anyhow!(
            "Failed to create cells storage dir {:?}: {}",
            config.cells_storage_dir,
            e
        )
    })?;

    let blob_store = Arc::new(LocalBlobStore::new(&config.cells_storage_dir));
    let cell_manager = Arc::new(CellManager::new(
        &config.cells_dir,
        blob_store,
        config.cells_lease_ttl_secs,
    ));

    let state = server::AppState {
        cell_manager,
        data_dir: config.data_dir.clone(),
    };
    let app = server::create_router(state);

    let addr = format!("{}:{}", config.host, config.port);
    info!("Starting gitcell server on {}", addr);

    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .map_err(|e| anyhow::anyhow!("Failed to bind to {}: {}", addr, e))?;

    axum::serve(listener, app)
        .await
        .map_err(|e| anyhow::anyhow!("Server error: {}", e))?;

    Ok(())
}
