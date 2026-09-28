//! The trash bin.
//!
//! Deleting a node (`DELETE /api/nodes/{id}`) marks it trashed in place. It
//! and its subtree disappear from every normal route, including shares and
//! public links, but stay decryptable because keys are still wrapped under
//! the original parent. Items belong to the tree owner's trash, even when a
//! share recipient deleted them, and are purged after `--trash-days`.

use crate::routes::activity::{self, Event};
use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use thencloud_crypto::api::*;

use crate::AppState;
use crate::access;
use crate::auth::AuthUser;
use crate::db::{NodeRow, get_node};
use crate::error::{AppError, Result};
use crate::routes::nodes::delete_subtree;
use crate::util::*;

/// A trashed node owned by `user`, or 404.
async fn trashed_node(state: &AppState, user: &AuthUser, id: &str) -> Result<NodeRow> {
    let trashed: Option<bool> = sqlx::query_scalar(
        "SELECT trashed_at IS NOT NULL FROM nodes WHERE id = ? AND owner_id = ?",
    )
    .bind(id)
    .bind(&user.id)
    .fetch_optional(&state.db)
    .await?;
    if trashed != Some(true) {
        return Err(AppError::NotFound);
    }
    get_node(&state.db, id).await?.ok_or(AppError::NotFound)
}

pub async fn list(State(state): State<AppState>, user: AuthUser) -> Result<Json<Vec<TrashItem>>> {
    let rows: Vec<(String, i64, String)> = sqlx::query_as(
        "SELECT n.id, n.trashed_at, COALESCE(u.username, '') FROM nodes n \
         LEFT JOIN users u ON u.id = n.trashed_by \
         WHERE n.owner_id = ? AND n.trashed_at IS NOT NULL ORDER BY n.trashed_at DESC",
    )
    .bind(&user.id)
    .fetch_all(&state.db)
    .await?;
    let mut items = Vec::with_capacity(rows.len());
    for (id, trashed_at, trashed_by) in rows {
        let mut path = Vec::new();
        for nid in access::ancestors(&state.db, &id).await?.iter().rev() {
            path.push(
                get_node(&state.db, nid)
                    .await?
                    .ok_or(AppError::NotFound)?
                    .into_api(),
            );
        }
        let Some(node) = path.last().cloned() else {
            continue;
        };
        items.push(TrashItem {
            node,
            path,
            trashed_at,
            trashed_by,
        });
    }
    Ok(Json(items))
}

/// Take a node out of the trash, into its original folder or, if that is
/// gone (409 `parent_unavailable`), into `parent_id` with a re-wrapped key.
pub async fn restore(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<String>,
    Json(req): Json<RestoreTrashRequest>,
) -> Result<Json<Node>> {
    let node = trashed_node(&state, &user, &id).await?;
    if let Some(m) = &req.enc_metadata {
        check_metadata(m)?;
    }
    check_name_tag(&req.name_tag)?;
    let moved = req.parent_id.is_some();
    let (parent, enc_key) = match (req.parent_id, req.enc_key) {
        (Some(p), Some(k)) => {
            check_id(&p, "parent_id")?;
            check_len(&k, WRAPPED_KEY_LEN, "enc_key")?;
            let target = get_node(&state.db, &p).await?.ok_or(AppError::NotFound)?;
            if target.owner_id != user.id || !target.is_folder() {
                return Err(AppError::bad("restore target must be one of your folders"));
            }
            if access::is_trashed(&state.db, &p).await? {
                return Err(AppError::ParentUnavailable);
            }
            if access::is_within(&state.db, &p, &id).await? {
                return Err(AppError::bad("cannot restore a folder into itself"));
            }
            (p, Some(k.0))
        }
        (None, None) => {
            let p = node
                .parent_id
                .clone()
                .ok_or_else(|| AppError::bad("the root folder can't be trashed"))?;
            if access::is_trashed(&state.db, &p).await? {
                return Err(AppError::ParentUnavailable);
            }
            (p, None)
        }
        _ => {
            return Err(AppError::bad(
                "parent_id and enc_key must be given together",
            ));
        }
    };
    // A new parent or name needs a new name tag; in place, the old one holds.
    let retag = moved || req.enc_metadata.is_some() || req.name_tag.is_some();
    sqlx::query(
        "UPDATE nodes SET trashed_at = NULL, trashed_by = NULL, parent_id = ?, \
         enc_key = COALESCE(?, enc_key), enc_metadata = COALESCE(?, enc_metadata), \
         name_tag = CASE WHEN ? THEN ? ELSE name_tag END, revision = revision + 1, updated_at = ? WHERE id = ?",
    )
    .bind(&parent)
    .bind(enc_key)
    .bind(req.enc_metadata.as_ref().map(|m| m.0.clone()))
    .bind(retag)
    .bind(req.name_tag.as_ref().map(|t| t.0.clone()))
    .bind(coarse_now())
    .bind(&id)
    .execute(&state.db)
    .await
    .map_err(crate::error::name_conflict)?;
    activity::note(&state.db, &user.id, &id, Event::Restored, None).await;
    Ok(Json(
        get_node(&state.db, &id)
            .await?
            .ok_or(AppError::NotFound)?
            .into_api(),
    ))
}

pub async fn purge(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<String>,
) -> Result<StatusCode> {
    trashed_node(&state, &user, &id).await?;
    delete_subtree(&state, &id, &user.id).await?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn empty(State(state): State<AppState>, user: AuthUser) -> Result<StatusCode> {
    let ids: Vec<String> =
        sqlx::query_scalar("SELECT id FROM nodes WHERE owner_id = ? AND trashed_at IS NOT NULL")
            .bind(&user.id)
            .fetch_all(&state.db)
            .await?;
    for id in ids {
        // A trashed node may already be gone as part of a trashed ancestor.
        if get_node(&state.db, &id).await?.is_some() {
            delete_subtree(&state, &id, &user.id).await?;
        }
    }
    Ok(StatusCode::NO_CONTENT)
}

/// Permanently delete everything trashed more than `days` ago.
pub async fn purge_expired(state: &AppState, days: i64) -> Result<usize> {
    let rows: Vec<(String, String)> = sqlx::query_as(
        "SELECT id, owner_id FROM nodes WHERE trashed_at IS NOT NULL AND trashed_at <= ?",
    )
    .bind(now() - days * 86400)
    .fetch_all(&state.db)
    .await?;
    let mut n = 0;
    for (id, owner) in rows {
        if get_node(&state.db, &id).await?.is_some() {
            delete_subtree(state, &id, &owner).await?;
            n += 1;
        }
    }
    Ok(n)
}
