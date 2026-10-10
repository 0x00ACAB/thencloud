//! thencloud server: a zero-knowledge file cloud.
//!
//! The server stores ciphertext, wrapped keys and a tree structure. It never
//! receives passwords, master keys, file keys or file names in the clear.

pub mod access;
pub mod audit;
pub mod auth;
pub mod blob;
pub mod config;
pub mod db;
pub mod downloader;
pub mod egress;
pub mod error;
pub mod janitor;
pub mod limiter;
pub mod maintenance;
pub mod routes;
pub mod s3;
pub mod settings;
pub mod setup;
pub mod snapshot;
pub mod totp;
pub mod transfer;
pub mod turnstile;
pub mod util;
pub mod webauthn;

use std::sync::Arc;

use sqlx::SqlitePool;

pub use config::Config;
pub use routes::router;

#[derive(Clone)]
pub struct AppState {
    pub db: SqlitePool,
    pub blobs: blob::BlobStore,
    pub config: Arc<Config>,
    /// Server-side secret for HMACs (fake prelogin salts, link tokens).
    pub secret: Arc<[u8; 32]>,
    pub limiter: Arc<limiter::Limiter>,
    /// Hash verified against when a user doesn't exist, so login timing
    /// doesn't reveal which usernames are registered.
    pub dummy_hash: Arc<String>,
    /// The optional video downloader (see downloader.rs).
    pub downloader: Arc<downloader::Downloader>,
    /// Changes to nodes, as they happen, for live updates (see routes/activity.rs).
    pub changes: tokio::sync::broadcast::Sender<routes::activity::Change>,
    /// Turnstile tokens already used (see turnstile.rs).
    pub turnstile_used: Arc<turnstile::Used>,
    /// Cancelled when the server starts shutting down: streams that would
    /// otherwise stay open for good (live updates) and the janitor stop.
    pub shutdown: tokio_util::sync::CancellationToken,
}

impl AppState {
    pub async fn new(config: Config) -> anyhow_like::Result<Self> {
        tokio::fs::create_dir_all(&config.data_dir).await?;
        let db = db::open(&config.data_dir.join("thencloud.db")).await?;
        let secret = db::server_secret(&db).await?;
        let blobs = match (s3::target_from_config(&config)?, config.s3_mirror) {
            (Some(target), true) => blob::BlobStore::mirror(config.data_dir.join("blobs"), target),
            (Some(target), false) => blob::BlobStore::s3(target),
            (None, true) => {
                return Err("--s3-mirror needs S3 configured (--s3-endpoint and the rest)".into());
            }
            (None, false) => blob::BlobStore::local(config.data_dir.join("blobs")),
        };
        let dummy_hash = util::hash_secret(b"thencloud-dummy".to_vec()).await?;
        let downloader = downloader::Downloader::new(
            config.yt_dlp.clone(),
            config.ffmpeg.clone(),
            &config.data_dir,
            config.downloader_public_only,
        )
        .await;
        Ok(AppState {
            db,
            blobs,
            config: Arc::new(config),
            secret: Arc::new(secret),
            limiter: Arc::new(limiter::Limiter::new(10, 15 * 60)),
            turnstile_used: Arc::default(),
            shutdown: tokio_util::sync::CancellationToken::new(),
            dummy_hash: Arc::new(dummy_hash),
            downloader: Arc::new(downloader),
            changes: tokio::sync::broadcast::channel(1024).0,
        })
    }
}

/// Minimal boxed-error result for startup code.
pub mod anyhow_like {
    pub type Result<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;
}
