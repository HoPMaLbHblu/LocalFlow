use std::net::SocketAddr;

use localflow::{
    api, db,
    db::repository::Repository,
    state::{AppState, Config},
};
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Load settings from a `.env` file if there is one.
    dotenvy::dotenv().ok();

    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("localflow=info")),
        )
        .init();

    let config = Config::from_env();

    let pool = db::connect(&config.database_url).await?;
    let repo = Repository::new(pool);

    let interrupted = repo.fail_interrupted_runs().await?;
    if interrupted > 0 {
        tracing::warn!(count = interrupted, "marked runs interrupted by the last shutdown as failed");
    }

    let state = AppState::new(repo, config.clone()).await?;
    state.scheduler.load_all(&state).await?;
    state.scheduler.start().await?;

    let addr = SocketAddr::new(config.host, config.port);
    let listener = tokio::net::TcpListener::bind(addr).await?;

    tracing::info!("LocalFlow is running at http://{addr}");
    for dir in state.path_policy.roots() {
        tracing::info!("scripts may access: {}", dir.display());
    }

    axum::serve(listener, api::routes::router(state))
        .with_graceful_shutdown(async {
            tokio::signal::ctrl_c().await.ok();
            tracing::info!("shutting down");
        })
        .await?;

    Ok(())
}
