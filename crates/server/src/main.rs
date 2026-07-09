use std::net::SocketAddr;

use localflow::{
    api,
    state::{AppState, Config},
};
use localflow_core::LocalFlow;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Load settings from a `.env` file if there is one.
    dotenvy::dotenv().ok();

    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| EnvFilter::new("localflow=info,localflow_core=info")),
        )
        .init();

    let config = Config::from_env();

    let flow = LocalFlow::open(config.core.clone(), None).await?;
    flow.start().await?;

    let addr = SocketAddr::new(config.host, config.port);
    let listener = tokio::net::TcpListener::bind(addr).await?;

    tracing::info!("LocalFlow is running at http://{addr}");
    for dir in flow.path_policy().roots() {
        tracing::info!("scripts may access: {}", dir.display());
    }

    axum::serve(listener, api::routes::router(AppState::new(flow)))
        .with_graceful_shutdown(async {
            tokio::signal::ctrl_c().await.ok();
            tracing::info!("shutting down");
        })
        .await?;

    Ok(())
}
