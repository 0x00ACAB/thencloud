//! Public-link tokens at rest.
//!
//! A link is found by the SHA-256 of its token. The token itself is kept
//! only sealed (`seal_link_token`) under a key in `<data dir>/link-token-key`,
//! outside the database, so the owner can be shown their link again but a
//! copy of the database alone (a backup, an S3 snapshot) holds no token
//! that opens anything. Without that file, links still open; only showing
//! the ones made before it was lost stops working.

use std::path::Path;

use sqlx::SqlitePool;
use thencloud_crypto::Key;

use crate::util::sha256;

const FILE: &str = "link-token-key";

/// What the `token` column holds once a row is sealed: unique, as its
/// constraint wants, and opening nothing.
pub fn placeholder(link_id: &str) -> String {
    format!("sealed:{link_id}")
}

pub fn hash(token: &str) -> Vec<u8> {
    sha256(token.as_bytes())
}

/// The key from `<data dir>/link-token-key`, made the first time.
pub fn load_key(data_dir: &Path) -> std::io::Result<Key> {
    crate::util::load_key_file(&data_dir.join(FILE))
}

/// Seal the tokens of links made before tokens were sealed. Runs at start;
/// does nothing once every row has a hash.
pub async fn seal_old(db: &SqlitePool, key: &Key) -> Result<u64, sqlx::Error> {
    let rows: Vec<(String, String)> =
        sqlx::query_as("SELECT id, token FROM public_links WHERE token_hash IS NULL")
            .fetch_all(db)
            .await?;
    let mut sealed = 0;
    for (id, token) in rows {
        sealed += sqlx::query(
            "UPDATE public_links SET token_hash = ?, enc_token = ?, token = ? \
             WHERE id = ? AND token_hash IS NULL",
        )
        .bind(hash(&token))
        .bind(thencloud_crypto::seal_link_token(key, &token, &id))
        .bind(placeholder(&id))
        .bind(&id)
        .execute(db)
        .await?
        .rows_affected();
    }
    if sealed > 0 {
        tracing::info!(links = sealed, "sealed the tokens of existing public links");
    }
    Ok(sealed)
}
