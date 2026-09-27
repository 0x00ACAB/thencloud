use std::net::SocketAddr;
use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use thencloud_server::{AppState, Config, janitor, maintenance, router};
use tracing_subscriber::EnvFilter;

#[derive(Parser)]
#[command(version, about = "thencloud: an end-to-end encrypted file cloud")]
struct Cli {
    #[command(flatten)]
    config: Config,
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    /// Serve thencloud (the default).
    Serve,
    /// Snapshot the database and the blobs it refers to into DIR, which
    /// must be new or empty. Safe while the server runs. DIR is a data
    /// directory of its own: to restore, stop the server and start it with
    /// --data-dir pointing at a copy of DIR.
    Backup { dir: PathBuf },
    /// Check that every blob the database expects exists with the right
    /// size, and list blob directories nothing refers to.
    Check,
}

#[tokio::main]
async fn main() -> Result<ExitCode, Box<dyn std::error::Error + Send + Sync>> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "thencloud_server=info,tower_http=info".into()),
        )
        .init();

    let Cli { config, command } = Cli::parse();
    match command {
        Some(Command::Backup { dir }) => return backup(config, dir).await,
        Some(Command::Check) => return check(config).await,
        Some(Command::Serve) | None => {}
    }
    if !config.web_dir.join("index.html").exists() {
        tracing::warn!(
            web_dir = %config.web_dir.display(),
            "web client not built; run ./build.sh to build it"
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
    Ok(ExitCode::SUCCESS)
}

/// Open an existing data directory without serving it.
async fn open(config: Config) -> Result<AppState, Box<dyn std::error::Error + Send + Sync>> {
    if !config.data_dir.join("thencloud.db").exists() {
        return Err(format!("no thencloud database in {}", config.data_dir.display()).into());
    }
    AppState::new(config).await
}

async fn backup(
    config: Config,
    dir: PathBuf,
) -> Result<ExitCode, Box<dyn std::error::Error + Send + Sync>> {
    let state = open(config).await?;
    let r = maintenance::backup(&state.db, &state.config.data_dir, &dir).await?;
    println!(
        "Backed up the database and {} chunks ({} bytes) to {}",
        r.chunks,
        r.bytes,
        dir.display()
    );
    if r.missing.is_empty() {
        return Ok(ExitCode::SUCCESS);
    }
    println!(
        "{} chunks were deleted while the backup ran; their versions can't be restored:",
        r.missing.len()
    );
    for (version, idx) in &r.missing {
        println!("  {version}/{idx}");
    }
    Ok(ExitCode::from(2))
}

async fn check(config: Config) -> Result<ExitCode, Box<dyn std::error::Error + Send + Sync>> {
    let state = open(config).await?;
    let r = maintenance::check(&state.db, &state.config.data_dir).await?;
    println!("Checked {} versions, {} chunks", r.versions, r.chunks);
    for (version, idx) in &r.missing {
        println!("missing: {version}/{idx}");
    }
    for (what, expected, found) in &r.wrong_size {
        println!("wrong size: {what} (expected {expected} bytes, found {found})");
    }
    for dir in &r.orphans {
        println!("not referred to: blobs/{}/{dir}", &dir[..2.min(dir.len())]);
    }
    if r.is_ok() {
        println!("Everything the database expects is there.");
        Ok(ExitCode::SUCCESS)
    } else {
        Ok(ExitCode::FAILURE)
    }
}
