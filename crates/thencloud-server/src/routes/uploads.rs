//! Chunked uploads of encrypted file content.
//!
//! 1. `POST /api/uploads` declares the new file (or new version) with its
//!    wrapped keys, encrypted metadata and chunk count.
//! 2. `PUT /api/uploads/{id}/chunks/{idx}` stores each encrypted chunk (any
//!    order, retries allowed). Quota is charged as chunks arrive.
//! 3. `POST /api/uploads/{id}/finish` atomically publishes the version.

use axum::Json;
use axum::body::Bytes;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use thencloud_crypto::api::*;
use thencloud_crypto::{MAX_ENCRYPTED_CHUNK, NONCE_LEN, TAG_LEN};

use crate::AppState;
use crate::access::{self, Access};
use crate::auth::AuthUser;
use crate::db::get_node;
use crate::error::{AppError, Result, is_fk_violation, is_unique_violation};
use crate::routes::versions;
use crate::util::*;

/// 1M chunks of 4 MiB = 4 TiB per file.
const MAX_CHUNKS: u32 = 1 << 20;

#[derive(sqlx::FromRow)]
struct UploadRow {
    id: String,
    owner_id: String,
    node_id: String,
    parent_id: Option<String>,
    enc_key: Option<Vec<u8>>,
    enc_metadata: Vec<u8>,
    version_id: String,
    enc_content_key: Vec<u8>,
    chunk_count: i64,
    if_revision: Option<i64>,
}

async fn load_upload(state: &AppState, id: &str, user_id: &str) -> Result<UploadRow> {
    sqlx::query_as(
        "SELECT id, owner_id, node_id, parent_id, enc_key, enc_metadata, version_id, enc_content_key, \
         chunk_count, if_revision FROM uploads WHERE id = ? AND user_id = ? AND expires_at > ?",
    )
    .bind(id)
    .bind(user_id)
    .bind(now())
    .fetch_optional(&state.db)
    .await?
    .ok_or(AppError::NotFound)
}

/// Check the caller may (still) write the target of an upload; returns the
/// owner of the tree being written to.
async fn check_target(
    state: &AppState,
    user: &AuthUser,
    node_id: &str,
    parent_id: Option<&str>,
) -> Result<String> {
    if let Some(parent) = parent_id {
        access::require(&state.db, &user.id, parent, Access::Write).await?;
        let p = get_node(&state.db, parent)
            .await?
            .ok_or(AppError::NotFound)?;
        if !p.is_folder() {
            return Err(AppError::bad("parent is not a folder"));
        }
        Ok(p.owner_id)
    } else {
        access::require(&state.db, &user.id, node_id, Access::Write).await?;
        let n = get_node(&state.db, node_id)
            .await?
            .ok_or(AppError::NotFound)?;
        if n.is_folder() {
            return Err(AppError::bad("cannot upload content to a folder"));
        }
        Ok(n.owner_id)
    }
}

pub async fn create(
    State(state): State<AppState>,
    user: AuthUser,
    Json(req): Json<CreateUploadRequest>,
) -> Result<(StatusCode, Json<UploadResponse>)> {
    check_id(&req.node_id, "node_id")?;
    check_id(&req.version_id, "version_id")?;
    check_metadata(&req.enc_metadata)?;
    check_len(&req.enc_content_key, WRAPPED_KEY_LEN, "enc_content_key")?;
    if req.chunk_count == 0 || req.chunk_count > MAX_CHUNKS {
        return Err(AppError::bad(format!(
            "chunk_count must be 1..={MAX_CHUNKS}"
        )));
    }
    match (&req.parent_id, &req.enc_key) {
        (Some(p), Some(k)) => {
            check_id(p, "parent_id")?;
            check_len(k, WRAPPED_KEY_LEN, "enc_key")?;
        }
        (Some(_), None) => return Err(AppError::bad("enc_key is required for a new file")),
        (None, Some(_)) => return Err(AppError::bad("enc_key is only accepted for new files")),
        (None, None) => {}
    }

    let owner_id = check_target(&state, &user, &req.node_id, req.parent_id.as_deref()).await?;
    let existing = get_node(&state.db, &req.node_id).await?;
    match (&req.parent_id, existing) {
        (Some(_), Some(_)) => return Err(AppError::Conflict("node id already exists".into())),
        (None, Some(n)) if req.if_revision.is_some_and(|r| r != n.revision) => {
            return Err(AppError::Conflict(
                "the file was modified by someone else".into(),
            ));
        }
        _ => {}
    }
    let taken: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM file_versions WHERE id = ?)")
        .bind(&req.version_id)
        .fetch_one(&state.db)
        .await?;
    if taken {
        return Err(AppError::Conflict("version id already exists".into()));
    }

    let id = new_uuid();
    let t = now();
    let expires_at = t + state.config.upload_ttl_hours * 3600;
    let res = sqlx::query(
        "INSERT INTO uploads (id, user_id, owner_id, node_id, parent_id, enc_key, enc_metadata, version_id, \
         enc_content_key, chunk_count, if_revision, created_at, expires_at) VALUES (?,?,?,?,?,?,?,?,?,?,?,?,?)",
    )
    .bind(&id)
    .bind(&user.id)
    .bind(&owner_id)
    .bind(&req.node_id)
    .bind(&req.parent_id)
    .bind(req.enc_key.as_ref().map(|k| k.0.clone()))
    .bind(&req.enc_metadata.0)
    .bind(&req.version_id)
    .bind(&req.enc_content_key.0)
    .bind(i64::from(req.chunk_count))
    .bind(req.if_revision)
    .bind(t)
    .bind(expires_at)
    .execute(&state.db)
    .await;
    match res {
        Err(e) if is_unique_violation(&e) => {
            return Err(AppError::Conflict("version id already exists".into()));
        }
        r => r?,
    };
    Ok((
        StatusCode::CREATED,
        Json(UploadResponse {
            upload_id: id,
            max_chunk_bytes: MAX_ENCRYPTED_CHUNK,
            expires_at,
        }),
    ))
}

/// Atomically charge `delta` bytes against the owner's quota.
async fn charge(state: &AppState, owner_id: &str, delta: i64) -> Result<()> {
    let res = sqlx::query(
        "UPDATE users SET used_bytes = MAX(0, used_bytes + ?) WHERE id = ? AND (? <= 0 OR used_bytes + ? <= quota_bytes)",
    )
    .bind(delta)
    .bind(owner_id)
    .bind(delta)
    .bind(delta)
    .execute(&state.db)
    .await?;
    if res.rows_affected() == 0 {
        return Err(AppError::QuotaExceeded);
    }
    Ok(())
}

/// Charge the quota; if it's full, make room by deleting the owner's oldest
/// old file versions (never current files or the trash) and try again.
async fn charge_or_prune(state: &AppState, owner_id: &str, delta: i64) -> Result<()> {
    match charge(state, owner_id, delta).await {
        Err(AppError::QuotaExceeded) => {
            let (used, quota): (i64, i64) =
                sqlx::query_as("SELECT used_bytes, quota_bytes FROM users WHERE id = ?")
                    .bind(owner_id)
                    .fetch_one(&state.db)
                    .await?;
            let needed = used + delta - quota;
            if versions::prune_for_space(state, owner_id, needed).await? < needed {
                return Err(AppError::QuotaExceeded);
            }
            charge(state, owner_id, delta).await
        }
        r => r,
    }
}

pub async fn put_chunk(
    State(state): State<AppState>,
    user: AuthUser,
    Path((id, idx)): Path<(String, u32)>,
    body: Bytes,
) -> Result<StatusCode> {
    let up = load_upload(&state, &id, &user.id).await?;
    if i64::from(idx) >= up.chunk_count {
        return Err(AppError::bad("chunk index out of range"));
    }
    if body.len() < NONCE_LEN + TAG_LEN || body.len() > MAX_ENCRYPTED_CHUNK {
        return Err(AppError::bad("chunk has an invalid size"));
    }
    let old: Option<i64> =
        sqlx::query_scalar("SELECT size FROM upload_chunks WHERE upload_id = ? AND idx = ?")
            .bind(&id)
            .bind(idx)
            .fetch_optional(&state.db)
            .await?;
    let delta = body.len() as i64 - old.unwrap_or(0);
    charge_or_prune(&state, &up.owner_id, delta).await?;

    let recorded = async {
        state.blobs.put_chunk(&up.version_id, idx, &body).await?;
        let mut tx = state.db.begin().await?;
        sqlx::query(
            "INSERT INTO upload_chunks (upload_id, idx, size) VALUES (?, ?, ?) \
             ON CONFLICT (upload_id, idx) DO UPDATE SET size = excluded.size",
        )
        .bind(&id)
        .bind(idx)
        .bind(body.len() as i64)
        .execute(&mut *tx)
        .await?;
        sqlx::query("UPDATE uploads SET received_bytes = received_bytes + ? WHERE id = ?")
            .bind(delta)
            .bind(&id)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok::<_, AppError>(())
    }
    .await;
    if let Err(e) = recorded {
        charge(&state, &up.owner_id, -delta).await.ok();
        return Err(match e {
            AppError::Db(ref d) if is_fk_violation(d) => AppError::NotFound,
            e => e,
        });
    }
    Ok(StatusCode::NO_CONTENT)
}

pub async fn finish(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<String>,
) -> Result<Json<Node>> {
    let up = load_upload(&state, &id, &user.id).await?;
    let (count, total): (i64, i64) = sqlx::query_as(
        "SELECT COUNT(*), COALESCE(SUM(size), 0) FROM upload_chunks WHERE upload_id = ?",
    )
    .bind(&id)
    .fetch_one(&state.db)
    .await?;
    if count != up.chunk_count {
        return Err(AppError::bad(format!(
            "only {count} of {} chunks were uploaded",
            up.chunk_count
        )));
    }
    // Permissions may have changed since the upload started.
    check_target(&state, &user, &up.node_id, up.parent_id.as_deref()).await?;

    let t = now();
    let mut tx = state.db.begin().await?;
    if let Some(parent) = &up.parent_id {
        let res = sqlx::query(
            "INSERT INTO nodes (id, owner_id, created_by, parent_id, kind, enc_key, enc_metadata, \
             current_version_id, created_at, updated_at) VALUES (?, ?, ?, ?, 'file', ?, ?, ?, ?, ?)",
        )
        .bind(&up.node_id)
        .bind(&up.owner_id)
        .bind(&user.id)
        .bind(parent)
        .bind(&up.enc_key)
        .bind(&up.enc_metadata)
        .bind(&up.version_id)
        .bind(t)
        .bind(t)
        .execute(&mut *tx)
        .await;
        match res {
            Err(e) if is_unique_violation(&e) => {
                return Err(AppError::Conflict("node id already exists".into()));
            }
            Err(e) if is_fk_violation(&e) => {
                return Err(AppError::Conflict(
                    "the parent folder no longer exists".into(),
                ));
            }
            r => r?,
        };
    } else {
        let rev: i64 = sqlx::query_scalar("SELECT revision FROM nodes WHERE id = ?")
            .bind(&up.node_id)
            .fetch_optional(&mut *tx)
            .await?
            .ok_or(AppError::NotFound)?;
        if up.if_revision.is_some_and(|r| r != rev) {
            return Err(AppError::Conflict(
                "the file was modified by someone else".into(),
            ));
        }
        let res = sqlx::query(
            "UPDATE nodes SET enc_metadata = ?, current_version_id = ?, revision = revision + 1, updated_at = ? \
             WHERE id = ? AND revision = ?",
        )
        .bind(&up.enc_metadata)
        .bind(&up.version_id)
        .bind(t)
        .bind(&up.node_id)
        .bind(rev)
        .execute(&mut *tx)
        .await?;
        if res.rows_affected() == 0 {
            return Err(AppError::Conflict(
                "the file was modified by someone else".into(),
            ));
        }
    }
    sqlx::query(
        "INSERT INTO file_versions (id, node_id, enc_content_key, enc_metadata, chunk_count, size, \
         created_by, created_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&up.version_id)
    .bind(&up.node_id)
    .bind(&up.enc_content_key)
    .bind(&up.enc_metadata)
    .bind(up.chunk_count)
    .bind(total)
    .bind(&user.id)
    .bind(t)
    .execute(&mut *tx)
    .await?;
    // The previous version stays in the history; drop any beyond the limit.
    let excess = versions::excess_versions(&mut tx, &up.node_id, state.config.max_versions).await?;
    versions::remove_versions(&mut tx, &up.owner_id, &excess).await?;
    sqlx::query("DELETE FROM uploads WHERE id = ?")
        .bind(&up.id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;

    for (v, _) in excess {
        state.blobs.delete_version(&v).await;
    }
    Ok(Json(
        get_node(&state.db, &up.node_id)
            .await?
            .ok_or(AppError::NotFound)?
            .into_api(),
    ))
}

pub async fn abort(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<String>,
) -> Result<StatusCode> {
    let up = load_upload(&state, &id, &user.id).await?;
    discard(&state, &up.id).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// Remove an unfinished upload, its chunks and its quota charge.
pub async fn discard(state: &AppState, upload_id: &str) -> Result<()> {
    let mut tx = state.db.begin().await?;
    let row: Option<(String, String, i64)> =
        sqlx::query_as("SELECT owner_id, version_id, received_bytes FROM uploads WHERE id = ?")
            .bind(upload_id)
            .fetch_optional(&mut *tx)
            .await?;
    let Some((owner_id, version_id, bytes)) = row else {
        return Ok(());
    };
    sqlx::query("DELETE FROM uploads WHERE id = ?")
        .bind(upload_id)
        .execute(&mut *tx)
        .await?;
    sqlx::query("UPDATE users SET used_bytes = MAX(0, used_bytes - ?) WHERE id = ?")
        .bind(bytes)
        .bind(&owner_id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    state.blobs.delete_version(&version_id).await;
    Ok(())
}
