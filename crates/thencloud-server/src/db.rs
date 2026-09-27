use std::path::Path;
use std::time::Duration;

use sqlx::SqlitePool;
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous};
use thencloud_crypto::api::{B64, Node, NodeKind, VersionInfo};

use crate::error::Result;

pub async fn open(
    path: &Path,
) -> std::result::Result<SqlitePool, Box<dyn std::error::Error + Send + Sync>> {
    let opts = SqliteConnectOptions::new()
        .filename(path)
        .create_if_missing(true)
        .journal_mode(SqliteJournalMode::Wal)
        .synchronous(SqliteSynchronous::Normal)
        .foreign_keys(true)
        .busy_timeout(Duration::from_secs(15));
    let pool = SqlitePoolOptions::new()
        .max_connections(8)
        .connect_with(opts)
        .await?;
    sqlx::migrate!("./migrations").run(&pool).await?;
    Ok(pool)
}

/// Load (or create on first start) the server's HMAC secret.
pub async fn server_secret(db: &SqlitePool) -> Result<[u8; 32]> {
    let fresh = thencloud_crypto::random_bytes(32);
    sqlx::query("INSERT OR IGNORE INTO server_secrets (name, value) VALUES ('hmac', ?)")
        .bind(&fresh)
        .execute(db)
        .await?;
    let v: Vec<u8> = sqlx::query_scalar("SELECT value FROM server_secrets WHERE name = 'hmac'")
        .fetch_one(db)
        .await?;
    Ok(v.try_into().expect("32-byte secret"))
}

/// Columns needed to build an [`api::Node`](Node).
pub const NODE_SELECT: &str = "SELECT n.id, n.parent_id, n.kind, n.owner_id, u.username AS owner, \
     n.enc_key, n.enc_metadata, n.revision, n.created_at, n.updated_at, \
     v.id AS v_id, v.enc_content_key AS v_key, v.chunk_count AS v_chunks, v.size AS v_size, v.created_at AS v_created \
     FROM nodes n JOIN users u ON u.id = n.owner_id \
     LEFT JOIN file_versions v ON v.id = n.current_version_id";

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct NodeRow {
    pub id: String,
    pub parent_id: Option<String>,
    pub kind: String,
    pub owner_id: String,
    pub owner: String,
    pub enc_key: Vec<u8>,
    pub enc_metadata: Vec<u8>,
    pub revision: i64,
    pub created_at: i64,
    pub updated_at: i64,
    pub v_id: Option<String>,
    pub v_key: Option<Vec<u8>>,
    pub v_chunks: Option<i64>,
    pub v_size: Option<i64>,
    pub v_created: Option<i64>,
}

impl NodeRow {
    pub fn is_folder(&self) -> bool {
        self.kind == "folder"
    }

    pub fn into_api(self) -> Node {
        let version = match (
            self.v_id,
            self.v_key,
            self.v_chunks,
            self.v_size,
            self.v_created,
        ) {
            (Some(id), Some(key), Some(chunks), Some(size), Some(created_at)) => {
                Some(VersionInfo {
                    id,
                    enc_content_key: B64(key),
                    chunk_count: chunks as u32,
                    size,
                    created_at,
                })
            }
            _ => None,
        };
        Node {
            id: self.id,
            parent_id: self.parent_id,
            kind: if self.kind == "folder" {
                NodeKind::Folder
            } else {
                NodeKind::File
            },
            owner: self.owner,
            enc_key: B64(self.enc_key),
            enc_metadata: B64(self.enc_metadata),
            revision: self.revision,
            created_at: self.created_at,
            updated_at: self.updated_at,
            version,
        }
    }
}

pub async fn get_node<'e, E: sqlx::SqliteExecutor<'e>>(e: E, id: &str) -> Result<Option<NodeRow>> {
    let sql = format!("{NODE_SELECT} WHERE n.id = ?");
    Ok(sqlx::query_as::<_, NodeRow>(&sql)
        .bind(id)
        .fetch_optional(e)
        .await?)
}

pub async fn get_children(db: &SqlitePool, parent_id: &str) -> Result<Vec<NodeRow>> {
    let sql = format!("{NODE_SELECT} WHERE n.parent_id = ? ORDER BY n.kind DESC, n.created_at");
    Ok(sqlx::query_as::<_, NodeRow>(&sql)
        .bind(parent_id)
        .fetch_all(db)
        .await?)
}
