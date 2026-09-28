//! Files dropped through upload-only links.
//!
//! A visitor to a file drop can't read the folder, so the new file's key is
//! sealed to the owner's public key instead of wrapped under the folder key.
//! Such a node is hidden everywhere (like a trashed one) until the owner's
//! client opens the sealed key, wraps it under the folder key and adopts
//! it here. Only the owner, who alone can open the key, reaches these.

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use sqlx::AssertSqlSafe;
use thencloud_crypto::api::*;

use crate::AppState;
use crate::access;
use crate::auth::AuthUser;
use crate::db::{NODE_SELECT, NodeRow};
use crate::error::{AppError, Result};
use crate::routes::nodes::delete_subtree;
use crate::util::*;

async fn dropped_node(state: &AppState, user: &AuthUser, id: &str) -> Result<NodeRow> {
    let sql = format!("{NODE_SELECT} WHERE n.id = ? AND n.owner_id = ? AND n.dropped = 1");
    sqlx::query_as(AssertSqlSafe(sql))
        .bind(id)
        .bind(&user.id)
        .fetch_optional(&state.db)
        .await?
        .ok_or(AppError::NotFound)
}

/// Dropped files waiting to be taken in, except those in trashed folders.
pub async fn list(State(state): State<AppState>, user: AuthUser) -> Result<Json<Vec<DroppedFile>>> {
    let sql = format!("{NODE_SELECT} WHERE n.owner_id = ? AND n.dropped = 1 ORDER BY n.created_at");
    let rows: Vec<NodeRow> = sqlx::query_as(AssertSqlSafe(sql))
        .bind(&user.id)
        .fetch_all(&state.db)
        .await?;
    let mut out = Vec::with_capacity(rows.len());
    for r in rows {
        let parent = r.parent_id.clone().unwrap_or_default();
        if access::is_trashed(&state.db, &parent).await? {
            continue;
        }
        let sealed_key = B64(r.enc_key.clone());
        out.push(DroppedFile {
            node: r.into_api(),
            sealed_key,
        });
    }
    Ok(Json(out))
}

pub async fn adopt(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<String>,
    Json(req): Json<AdoptDropRequest>,
) -> Result<Json<Node>> {
    check_len(&req.enc_key, WRAPPED_KEY_LEN, "enc_key")?;
    if let Some(m) = &req.enc_metadata {
        check_metadata(m)?;
    }
    check_name_tag(&req.name_tag)?;
    dropped_node(&state, &user, &id).await?;
    let mut tx = state.db.begin().await?;
    // A fresh node key: its content key is re-wrapped under it, so later
    // versions aren't readable with the key the visitor chose.
    if let Some(ck) = &req.enc_content_key {
        check_len(ck, WRAPPED_KEY_LEN, "enc_content_key")?;
        let meta = req
            .enc_metadata
            .as_ref()
            .ok_or_else(|| AppError::bad("enc_content_key needs enc_metadata"))?;
        let r = sqlx::query(
            "UPDATE file_versions SET enc_content_key = ?, enc_metadata = ? WHERE node_id = ?",
        )
        .bind(&ck.0)
        .bind(&meta.0)
        .bind(&id)
        .execute(&mut *tx)
        .await?;
        if r.rows_affected() != 1 {
            return Err(AppError::bad("a dropped file has exactly one version"));
        }
    }
    sqlx::query(
        "UPDATE nodes SET enc_key = ?, enc_metadata = COALESCE(?, enc_metadata), name_tag = ?, dropped = 0, \
         revision = revision + 1 WHERE id = ? AND dropped = 1",
    )
    .bind(&req.enc_key.0)
    .bind(req.enc_metadata.as_ref().map(|m| m.0.clone()))
    .bind(req.name_tag.as_ref().map(|t| t.0.clone()))
    .bind(&id)
    .execute(&mut *tx)
    .await
    .map_err(crate::error::name_conflict)?;
    tx.commit().await?;
    Ok(Json(
        crate::db::get_node(&state.db, &id)
            .await?
            .ok_or(AppError::NotFound)?
            .into_api(),
    ))
}

/// Throw away a dropped file without taking it in (for example one whose
/// key doesn't open).
pub async fn discard(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<String>,
) -> Result<StatusCode> {
    dropped_node(&state, &user, &id).await?;
    delete_subtree(&state, &id, &user.id).await?;
    Ok(StatusCode::NO_CONTENT)
}
