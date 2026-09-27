use std::net::SocketAddr;

use clap::Parser;
use thencloud_server::{AppState, Config, janitor, router};
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "thencloud_server=info,tower_http=info".into()),
        )
        .init();

    let config = Config::parse();
    if !config.web_dir.join("pkg/thencloud_wasm.js").exists() {
        tracing::warn!(
            web_dir = %config.web_dir.display(),
            "web client WASM not found; run ./build.sh to build it"
        );
    }

    let state = AppState::new(config.clone()).await?;
    janitor::spawn(state.clone());

    let listener = tokio::net::TcpListener::bind(config.bind).await?;
    tracing::info!("thencloud listening on http://{}", listener.local_addr()?);
    axum::serve(
        listener,
        router(state).into_make_service_with_connect_info::<SocketAddr>(),
    )
    .with_graceful_shutdown(async {
        let _ = tokio::signal::ctrl_c().await;
        tracing::info!("shutting down");
    })
    .await?;
    Ok(())
}
