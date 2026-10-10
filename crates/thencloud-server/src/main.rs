use std::net::SocketAddr;
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use thencloud_server::{AppState, Config, janitor, maintenance, router, snapshot};
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
    /// Snapshot the database and the blobs it refers to into DEST, which
    /// must be new or empty: a directory, or s3://BUCKET/PREFIX (using the
    /// configured S3 endpoint and credentials). Safe while the server runs.
    /// A directory DEST is a data directory of its own: to restore, stop
    /// the server and start it with --data-dir pointing at a copy of DEST.
    /// To restore an S3 DEST, put its thencloud.db in a data directory and
    /// point --s3-prefix at the backup's blobs/ prefix.
    Backup { dest: String },
    /// Check that every blob the database expects exists with the right
    /// size, and list blob directories nothing refers to. With
    /// --s3-mirror, each copy is checked on its own.
    Check,
    /// Put the newest database snapshot from the S3 bucket (see
    /// --s3-snapshot-hours) into the data directory, which must not have a
    /// database yet. With the same S3 options, the server then starts as it
    /// was at that snapshot.
    RestoreSnapshot,
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
        Some(Command::Backup { dest }) => return backup(config, dest).await,
        Some(Command::Check) => return check(config).await,
        Some(Command::RestoreSnapshot) => return restore_snapshot(config).await,
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
    thencloud_server::setup::prepare(&state).await?;
    thencloud_server::settings::warn_if_open(&state).await?;
    snapshot::spawn(state.clone());

    let listener = tokio::net::TcpListener::bind(config.bind).await?;
    tracing::info!("thencloud listening on http://{}", listener.local_addr()?);
    let shutdown = state.shutdown.clone();
    let server = axum::serve(
        listener,
        router(state).into_make_service_with_connect_info::<SocketAddr>(),
    )
    .with_graceful_shutdown(shutdown.clone().cancelled_owned());
    // On Ctrl+C or SIGTERM (docker stop): stop taking connections, end the
    // live-update streams, and give requests under way a moment to finish.
    let deadline = async {
        stop_signal().await;
        tracing::info!("shutting down");
        shutdown.cancel();
        tokio::time::sleep(SHUTDOWN_GRACE).await;
    };
    tokio::select! {
        r = server => r?,
        () = deadline => tracing::warn!(
            "requests still running after {} s; stopping anyway",
            SHUTDOWN_GRACE.as_secs()
        ),
    }
    Ok(ExitCode::SUCCESS)
}

/// How long requests under way (downloads, uploads) get to finish.
const SHUTDOWN_GRACE: std::time::Duration = std::time::Duration::from_secs(10);

/// Ctrl+C, or SIGTERM where there is one.
async fn stop_signal() {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{SignalKind, signal};
        match signal(SignalKind::terminate()) {
            Ok(mut term) => {
                tokio::select! {
                    _ = tokio::signal::ctrl_c() => {}
                    _ = term.recv() => {}
                }
            }
            Err(_) => {
                let _ = tokio::signal::ctrl_c().await;
            }
        }
    }
    #[cfg(not(unix))]
    {
        let _ = tokio::signal::ctrl_c().await;
    }
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
    dest: String,
) -> Result<ExitCode, Box<dyn std::error::Error + Send + Sync>> {
    let parsed = maintenance::BackupDest::parse(&dest, &config)?;
    let state = open(config).await?;
    let r = maintenance::backup(&state.db, &state.blobs, &parsed).await?;
    println!(
        "Backed up the database and {} chunks ({} bytes) to {}",
        r.chunks, r.bytes, dest
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
    let copies = state.blobs.copies();
    let mut ok = true;
    for store in &copies {
        if copies.len() > 1 {
            println!("In {}:", store.describe());
        }
        let r = maintenance::check(&state.db, store).await?;
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
        }
        ok &= r.is_ok();
    }
    Ok(if ok {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    })
}

async fn restore_snapshot(
    config: Config,
) -> Result<ExitCode, Box<dyn std::error::Error + Send + Sync>> {
    let target = thencloud_server::s3::target_from_config(&config)?
        .ok_or("restore-snapshot needs the S3 options (--s3-endpoint and the rest)")?;
    let (key, at) = snapshot::restore(&target, &config.data_dir).await?;
    println!(
        "Restored {key} ({} seconds old) as {}",
        thencloud_server::util::now() - at,
        config.data_dir.join("thencloud.db").display()
    );
    Ok(ExitCode::SUCCESS)
}
