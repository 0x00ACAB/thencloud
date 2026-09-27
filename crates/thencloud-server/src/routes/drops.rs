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
    sqlx::query_as(&sql)
        .bind(id)
        .bind(&user.id)
        .fetch_optional(&state.db)
        .await?
        .ok_or(AppError::NotFound)
}

/// Dropped files waiting to be taken in, except those in trashed folders.
pub async fn list(State(state): State<AppState>, user: AuthUser) -> Result<Json<Vec<DroppedFile>>> {
    let sql = format!("{NODE_SELECT} WHERE n.owner_id = ? AND n.dropped = 1 ORDER BY n.created_at");
    let rows: Vec<NodeRow> = sqlx::query_as(&sql)
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
    dropped_node(&state, &user, &id).await?;
    sqlx::query(
        "UPDATE nodes SET enc_key = ?, dropped = 0, revision = revision + 1 WHERE id = ? AND dropped = 1",
    )
    .bind(&req.enc_key.0)
    .bind(&id)
    .execute(&state.db)
    .await?;
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
