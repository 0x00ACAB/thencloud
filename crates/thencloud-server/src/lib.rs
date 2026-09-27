//! thencloud server: a zero-knowledge file cloud.
//!
//! The server stores ciphertext, wrapped keys and a tree structure. It never
//! receives passwords, master keys, file keys or file names in the clear.

pub mod access;
pub mod auth;
pub mod blob;
pub mod config;
pub mod db;
pub mod error;
pub mod janitor;
pub mod limiter;
pub mod routes;
pub mod util;

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
}

impl AppState {
    pub async fn new(config: Config) -> anyhow_like::Result<Self> {
        tokio::fs::create_dir_all(&config.data_dir).await?;
        let db = db::open(&config.data_dir.join("thencloud.db")).await?;
        let secret = db::server_secret(&db).await?;
        let blobs = blob::BlobStore::new(config.data_dir.join("blobs"));
        let dummy_hash = util::hash_secret(b"thencloud-dummy".to_vec()).await?;
        Ok(AppState {
            db,
            blobs,
            config: Arc::new(config),
            secret: Arc::new(secret),
            limiter: Arc::new(limiter::Limiter::new(10, 15 * 60)),
            dummy_hash: Arc::new(dummy_hash),
        })
    }
}

/// Minimal boxed-error result for startup code.
pub mod anyhow_like {
    pub type Result<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;
}
