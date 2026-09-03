use gitcell::{config::Config, error::Result, server, storage};
use sqlx::sqlite::SqlitePoolOptions;
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

    let pool = SqlitePoolOptions::new()
        .max_connections(5)
        .connect(&config.database_url)
        .await
        .map_err(|e| anyhow::anyhow!("Failed to connect to database: {}", e))?;

    storage::migrate(&pool).await?;

    let state = server::AppState {
        pool,
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
