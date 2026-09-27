//! Public links, managed by the owner.
//!
//! A link is `https://host/s/<token>#<node key>`. The server only knows the
//! token; the key after `#` is never sent in any HTTP request.

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use thencloud_crypto::api::*;

use crate::AppState;
use crate::access::{self, Access};
use crate::auth::AuthUser;
use crate::error::{AppError, Result};
use crate::routes::shares::NodeFilter;
use crate::util::*;

pub async fn create(
    State(state): State<AppState>,
    user: AuthUser,
    Json(req): Json<CreateLinkRequest>,
) -> Result<(StatusCode, Json<Link>)> {
    check_id(&req.node_id, "node_id")?;
    access::require(&state.db, &user.id, &req.node_id, Access::Owner).await?;
    if req.upload_only
        && !crate::db::get_node(&state.db, &req.node_id)
            .await?
            .is_some_and(|n| n.is_folder())
    {
        return Err(AppError::bad("upload-only links are for folders"));
    }
    let t = now();
    if req.expires_at.is_some_and(|e| e <= t) {
        return Err(AppError::bad("expires_at must be in the future"));
    }
    let password_hash = match req.password.as_deref().filter(|p| !p.is_empty()) {
        Some(p) if p.len() > 1024 => return Err(AppError::bad("password is too long")),
        Some(p) => Some(hash_secret(p.as_bytes().to_vec()).await?),
        None => None,
    };
    let link = Link {
        id: new_uuid(),
        token: random_token(18),
        node_id: req.node_id,
        has_password: password_hash.is_some(),
        expires_at: req.expires_at,
        created_at: t,
        upload_only: req.upload_only,
    };
    sqlx::query(
        "INSERT INTO public_links (id, token, node_id, owner_id, password_hash, expires_at, created_at, \
         upload_only) VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&link.id)
    .bind(&link.token)
    .bind(&link.node_id)
    .bind(&user.id)
    .bind(password_hash)
    .bind(link.expires_at)
    .bind(t)
    .bind(link.upload_only)
    .execute(&state.db)
    .await?;
    Ok((StatusCode::CREATED, Json(link)))
}

/// `(id, token, node_id, has_password, expires_at, created_at, upload_only)`
type LinkRow = (String, String, String, bool, Option<i64>, i64, bool);

pub async fn list(
    State(state): State<AppState>,
    user: AuthUser,
    Query(f): Query<NodeFilter>,
) -> Result<Json<Vec<Link>>> {
    let rows: Vec<LinkRow> = sqlx::query_as(
        "SELECT id, token, node_id, password_hash IS NOT NULL, expires_at, created_at, upload_only FROM public_links \
         WHERE owner_id = ? AND (? IS NULL OR node_id = ?) ORDER BY created_at",
    )
    .bind(&user.id)
    .bind(&f.node_id)
    .bind(&f.node_id)
    .fetch_all(&state.db)
    .await?;
    let mut visible = Vec::with_capacity(rows.len());
    for r in rows {
        if !access::is_trashed(&state.db, &r.2).await? {
            visible.push(r);
        }
    }
    Ok(Json(
        visible
            .into_iter()
            .map(
                |(id, token, node_id, has_password, expires_at, created_at, upload_only)| Link {
                    id,
                    token,
                    node_id,
                    has_password,
                    expires_at,
                    created_at,
                    upload_only,
                },
            )
            .collect(),
    ))
}

pub async fn delete(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<String>,
) -> Result<StatusCode> {
    let res = sqlx::query("DELETE FROM public_links WHERE id = ? AND owner_id = ?")
        .bind(&id)
        .bind(&user.id)
        .execute(&state.db)
        .await?;
    if res.rows_affected() == 0 {
        return Err(AppError::NotFound);
    }
    Ok(StatusCode::NO_CONTENT)
}
