use axum::Json;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use thencloud_crypto::api::*;

use crate::access::{self, Access};
use crate::auth::AuthUser;
use crate::db::{NodeRow, get_children, get_node};
use crate::error::{AppError, Result, is_unique_violation, name_conflict};
use crate::util::*;
use crate::{AppState, subtree_cte};

pub async fn get(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<String>,
) -> Result<Json<Node>> {
    access::require(&state.db, &user.id, &id, Access::Read).await?;
    let node = get_node(&state.db, &id).await?.ok_or(AppError::NotFound)?;
    Ok(Json(node.into_api()))
}

pub async fn children(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<String>,
) -> Result<Json<Vec<Node>>> {
    access::require(&state.db, &user.id, &id, Access::Read).await?;
    let node = get_node(&state.db, &id).await?.ok_or(AppError::NotFound)?;
    if !node.is_folder() {
        return Err(AppError::bad("not a folder"));
    }
    let kids = get_children(&state.db, &id).await?;
    Ok(Json(kids.into_iter().map(NodeRow::into_api).collect()))
}

/// The chain of nodes the caller needs to derive this node's key: from
/// their root folder (owner) or from the top-most node shared with them.
pub async fn path(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<String>,
) -> Result<Json<NodePath>> {
    let acc = access::require(&state.db, &user.id, &id, Access::Read).await?;
    let anc = access::ancestors(&state.db, &id).await?;
    let (top, share) = if acc == Access::Owner {
        (anc.len() - 1, None)
    } else {
        let shares = access::shares_on_path(&state.db, &user.id, &id).await?;
        let (share_id, node_id, wrapped_key, permission) =
            shares.into_iter().last().ok_or(AppError::NotFound)?;
        let top = anc
            .iter()
            .position(|a| *a == node_id)
            .ok_or(AppError::NotFound)?;
        let permission = if permission == "write" {
            Permission::Write
        } else {
            Permission::Read
        };
        (
            top,
            Some(ShareKey {
                share_id,
                wrapped_key: B64(wrapped_key),
                permission,
            }),
        )
    };
    let mut nodes = Vec::with_capacity(top + 1);
    for nid in anc[..=top].iter().rev() {
        nodes.push(
            get_node(&state.db, nid)
                .await?
                .ok_or(AppError::NotFound)?
                .into_api(),
        );
    }
    Ok(Json(NodePath { nodes, share }))
}

pub async fn create_folder(
    State(state): State<AppState>,
    user: AuthUser,
    Json(req): Json<CreateFolderRequest>,
) -> Result<(StatusCode, Json<Node>)> {
    check_id(&req.id, "id")?;
    check_id(&req.parent_id, "parent_id")?;
    check_len(&req.enc_key, WRAPPED_KEY_LEN, "enc_key")?;
    check_metadata(&req.enc_metadata)?;
    check_name_tag(&req.name_tag)?;
    access::require(&state.db, &user.id, &req.parent_id, Access::Write).await?;
    let parent = get_node(&state.db, &req.parent_id)
        .await?
        .ok_or(AppError::NotFound)?;
    if !parent.is_folder() {
        return Err(AppError::bad("parent is not a folder"));
    }
    let t = coarse_now();
    let res = sqlx::query(
        "INSERT INTO nodes (id, owner_id, created_by, parent_id, kind, enc_key, enc_metadata, created_at, updated_at, \
         name_tag) VALUES (?, ?, ?, ?, 'folder', ?, ?, ?, ?, ?)",
    )
    .bind(&req.id)
    .bind(&parent.owner_id)
    .bind(&user.id)
    .bind(&req.parent_id)
    .bind(&req.enc_key.0)
    .bind(&req.enc_metadata.0)
    .bind(t)
    .bind(t)
    .bind(req.name_tag.as_ref().map(|t| t.0.clone()))
    .execute(&state.db)
    .await;
    match res {
        Err(e) if is_unique_violation(&e) => {
            return Err(match name_conflict(e) {
                AppError::NameTaken => AppError::NameTaken,
                _ => AppError::Conflict("node id already exists".into()),
            });
        }
        r => r?,
    };
    let node = get_node(&state.db, &req.id)
        .await?
        .ok_or(AppError::NotFound)?;
    Ok((StatusCode::CREATED, Json(node.into_api())))
}

/// Structural changes (rename, move, delete) need write access to the
/// node's parent, so a share recipient can't rename or delete the shared
/// folder itself, only its contents.
async fn require_parent_write(state: &AppState, user: &AuthUser, node: &NodeRow) -> Result<String> {
    let parent = node
        .parent_id
        .clone()
        .ok_or_else(|| AppError::bad("the root folder cannot be modified"))?;
    match access::access(&state.db, &user.id, &parent).await? {
        Some(a) if a >= Access::Write => Ok(parent),
        _ => Err(AppError::Forbidden),
    }
}

pub async fn update(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<String>,
    Json(req): Json<UpdateNodeRequest>,
) -> Result<Json<Node>> {
    access::require(&state.db, &user.id, &id, Access::Read).await?;
    let node = get_node(&state.db, &id).await?.ok_or(AppError::NotFound)?;
    let parent = require_parent_write(&state, &user, &node).await?;

    if let Some(m) = &req.enc_metadata {
        check_metadata(m)?;
    }
    check_name_tag(&req.name_tag)?;
    let moving_to = req.parent_id.as_deref().filter(|p| *p != parent);
    let new_key = match (moving_to, &req.enc_key) {
        (Some(target), Some(key)) => {
            check_id(target, "parent_id")?;
            check_len(key, WRAPPED_KEY_LEN, "enc_key")?;
            access::require(&state.db, &user.id, target, Access::Write).await?;
            let t = get_node(&state.db, target)
                .await?
                .ok_or(AppError::NotFound)?;
            if !t.is_folder() {
                return Err(AppError::bad("target is not a folder"));
            }
            if t.owner_id != node.owner_id {
                return Err(AppError::bad("cannot move between different users' trees"));
            }
            if access::is_within(&state.db, target, &id).await? {
                return Err(AppError::bad("cannot move a folder into itself"));
            }
            Some(key.0.clone())
        }
        (Some(_), None) => {
            return Err(AppError::bad(
                "enc_key re-wrapped for the new parent is required when moving",
            ));
        }
        (None, Some(_)) => return Err(AppError::bad("enc_key can only be changed when moving")),
        (None, None) => None,
    };
    if req.enc_metadata.is_none() && new_key.is_none() {
        return Err(AppError::bad("nothing to update"));
    }

    let expected = req.if_revision.unwrap_or(node.revision);
    // A new tag replaces the old one. Without one, a rename or move makes
    // the old tag wrong, so it's cleared; anything else keeps it.
    let renamed_or_moved = req.enc_metadata.is_some() || new_key.is_some();
    let res = sqlx::query(
        "UPDATE nodes SET enc_metadata = COALESCE(?, enc_metadata), parent_id = COALESCE(?, parent_id), \
         enc_key = COALESCE(?, enc_key), \
         name_tag = CASE WHEN ? IS NOT NULL THEN ? WHEN ? THEN NULL ELSE name_tag END, \
         revision = revision + 1, updated_at = ? WHERE id = ? AND revision = ?",
    )
    .bind(req.enc_metadata.as_ref().map(|m| m.0.clone()))
    .bind(moving_to)
    .bind(new_key)
    .bind(req.name_tag.as_ref().map(|t| t.0.clone()))
    .bind(req.name_tag.as_ref().map(|t| t.0.clone()))
    .bind(renamed_or_moved)
    .bind(coarse_now())
    .bind(&id)
    .bind(expected)
    .execute(&state.db)
    .await
    .map_err(name_conflict)?;
    if res.rows_affected() == 0 {
        return Err(AppError::Conflict(
            "the node was modified by someone else; reload and retry".into(),
        ));
    }
    Ok(Json(
        get_node(&state.db, &id)
            .await?
            .ok_or(AppError::NotFound)?
            .into_api(),
    ))
}

/// Tag children made before name tags existed. Best effort: a tag that
/// clashes with another child's is skipped.
pub async fn tag_names(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<String>,
    Json(req): Json<NameTags>,
) -> Result<StatusCode> {
    access::require(&state.db, &user.id, &id, Access::Write).await?;
    for t in req.tags.iter().take(10_000) {
        check_len(&t.name_tag, NAME_TAG_LEN, "name_tag")?;
        let r = sqlx::query(
            "UPDATE nodes SET name_tag = ? WHERE id = ? AND parent_id = ? AND name_tag IS NULL",
        )
        .bind(&t.name_tag.0)
        .bind(&t.id)
        .bind(&id)
        .execute(&state.db)
        .await;
        match r.map_err(name_conflict) {
            Err(AppError::NameTaken) | Ok(_) => {}
            Err(e) => return Err(e),
        }
    }
    Ok(StatusCode::NO_CONTENT)
}

/// Move a node (and, implicitly, everything below it) to the owner's trash.
/// It stays in place in the tree, so its key still unwraps under its parent
/// and restoring needs no re-encryption. See `routes/trash.rs`.
pub async fn delete(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<String>,
) -> Result<StatusCode> {
    access::require(&state.db, &user.id, &id, Access::Read).await?;
    let node = get_node(&state.db, &id).await?.ok_or(AppError::NotFound)?;
    require_parent_write(&state, &user, &node).await?;
    sqlx::query(
        "UPDATE nodes SET trashed_at = ?, trashed_by = ?, revision = revision + 1 WHERE id = ?",
    )
    .bind(coarse_now())
    .bind(&user.id)
    .bind(&id)
    .execute(&state.db)
    .await?;
    Ok(StatusCode::NO_CONTENT)
}

/// Permanently delete a node and everything below it, including pending
/// uploads into it, and release the owner's quota.
pub async fn delete_subtree(state: &AppState, node_id: &str, owner_id: &str) -> Result<()> {
    let mut tx = state.db.begin().await?;
    let versions: Vec<(String, i64)> = sqlx::query_as(concat!(
        subtree_cte!(),
        "SELECT v.id, v.size FROM file_versions v JOIN sub ON v.node_id = sub.id"
    ))
    .bind(node_id)
    .fetch_all(&mut *tx)
    .await?;
    let uploads: Vec<(String, String, i64)> = sqlx::query_as(concat!(
        subtree_cte!(),
        "SELECT u.id, u.version_id, u.received_bytes FROM uploads u \
         WHERE u.node_id IN (SELECT id FROM sub) OR u.parent_id IN (SELECT id FROM sub)"
    ))
    .bind(node_id)
    .fetch_all(&mut *tx)
    .await?;
    for (uid, _, _) in &uploads {
        sqlx::query("DELETE FROM uploads WHERE id = ?")
            .bind(uid)
            .execute(&mut *tx)
            .await?;
    }
    // Children, versions, shares and links go via ON DELETE CASCADE.
    sqlx::query("DELETE FROM nodes WHERE id = ?")
        .bind(node_id)
        .execute(&mut *tx)
        .await?;
    let freed: i64 =
        versions.iter().map(|v| v.1).sum::<i64>() + uploads.iter().map(|u| u.2).sum::<i64>();
    sqlx::query("UPDATE users SET used_bytes = MAX(0, used_bytes - ?) WHERE id = ?")
        .bind(freed)
        .bind(owner_id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;

    let ids: Vec<String> = versions
        .into_iter()
        .map(|v| v.0)
        .chain(uploads.into_iter().map(|u| u.1))
        .collect();
    state.blobs.delete_versions(&ids).await;
    Ok(())
}

pub async fn chunk(
    State(state): State<AppState>,
    user: AuthUser,
    Path((id, idx)): Path<(String, u32)>,
) -> Result<Response> {
    access::require(&state.db, &user.id, &id, Access::Read).await?;
    let node = get_node(&state.db, &id).await?.ok_or(AppError::NotFound)?;
    current_chunk(&state, node, idx).await
}

/// Serve one chunk of a file's current version.
pub async fn current_chunk(state: &AppState, node: NodeRow, idx: u32) -> Result<Response> {
    if node.is_folder() {
        return Err(AppError::bad("not a file"));
    }
    let (Some(vid), Some(count)) = (node.v_id, node.v_chunks) else {
        return Err(AppError::NotFound);
    };
    chunk_response(state, &vid, count, idx).await
}

/// Serve one encrypted chunk of a version. The version id is returned in
/// `X-Version-Id` so the client can detect a concurrent update (decryption
/// would fail anyway, since chunks are bound to their version).
pub async fn chunk_response(
    state: &AppState,
    version_id: &str,
    chunk_count: i64,
    idx: u32,
) -> Result<Response> {
    if i64::from(idx) >= chunk_count {
        return Err(AppError::NotFound);
    }
    let data = state.blobs.get_chunk(version_id, idx).await.map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            AppError::Internal(format!("blob missing for version {version_id} chunk {idx}"))
        } else {
            AppError::Io(e)
        }
    })?;
    let mut h = HeaderMap::new();
    h.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/octet-stream"),
    );
    h.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    h.insert(
        "x-version-id",
        HeaderValue::from_str(version_id).map_err(|e| AppError::Internal(e.to_string()))?,
    );
    Ok((h, data).into_response())
}
