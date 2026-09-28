//! Public links, managed by the owner.
//!
//! A link is `https://host/s/<token>#<node key>`, or with a password
//! `#p.<secret>`, where the node key is only reachable with both the secret
//! and the password. The server only knows the token; what follows `#` is
//! never sent in any HTTP request.

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
    if req.max_opens.is_some_and(|n| !(1..=1_000_000).contains(&n)) {
        return Err(AppError::bad("max_opens must be between 1 and 1000000"));
    }
    if req.upload_only && req.max_opens.is_some() {
        return Err(AppError::bad(
            "upload-only links can't have a limit on opens",
        ));
    }
    let t = now();
    if req.expires_at.is_some_and(|e| e <= t) {
        return Err(AppError::bad("expires_at must be in the future"));
    }
    // A password comes as an auth key derived from it. Links to content
    // also carry the node key wrapped under the password (see
    // `derive_link_password_keys`); a file drop has nothing to wrap.
    let with_key = req.password_auth.is_some() && !req.upload_only;
    if req.password_auth.as_ref().is_some_and(|a| a.len() != 32) {
        return Err(AppError::bad("password_auth must be 32 bytes"));
    }
    for field in [&req.enc_link_key, &req.enc_link_secret] {
        match field {
            Some(k) if !with_key || k.len() > 256 => {
                return Err(AppError::bad(
                    "enc_link_key and enc_link_secret go with a password",
                ));
            }
            None if with_key => {
                return Err(AppError::bad(
                    "a link with a password needs enc_link_key and enc_link_secret",
                ));
            }
            _ => {}
        }
    }
    let password_hash = match &req.password_auth {
        Some(a) => Some(hash_secret(a.0.clone()).await?),
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
        max_opens: req.max_opens,
        opens: 0,
        enc_link_secret: req.enc_link_secret.clone(),
    };
    sqlx::query(
        "INSERT INTO public_links (id, token, node_id, owner_id, password_hash, expires_at, created_at, \
         upload_only, max_opens, enc_link_key, enc_link_secret) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&link.id)
    .bind(&link.token)
    .bind(&link.node_id)
    .bind(&user.id)
    .bind(password_hash)
    .bind(link.expires_at)
    .bind(t)
    .bind(link.upload_only)
    .bind(link.max_opens)
    .bind(req.enc_link_key.map(|k| k.0))
    .bind(req.enc_link_secret.map(|k| k.0))
    .execute(&state.db)
    .await?;
    Ok((StatusCode::CREATED, Json(link)))
}

#[derive(sqlx::FromRow)]
struct LinkRow {
    id: String,
    token: String,
    node_id: String,
    has_password: bool,
    expires_at: Option<i64>,
    created_at: i64,
    upload_only: bool,
    max_opens: Option<i64>,
    opens: i64,
    enc_link_secret: Option<Vec<u8>>,
}

pub async fn list(
    State(state): State<AppState>,
    user: AuthUser,
    Query(f): Query<NodeFilter>,
) -> Result<Json<Vec<Link>>> {
    let rows: Vec<LinkRow> = sqlx::query_as(
        "SELECT id, token, node_id, password_hash IS NOT NULL AS has_password, expires_at, created_at, upload_only, \
         max_opens, opens, enc_link_secret FROM public_links \
         WHERE owner_id = ? AND (? IS NULL OR node_id = ?) ORDER BY created_at",
    )
    .bind(&user.id)
    .bind(&f.node_id)
    .bind(&f.node_id)
    .fetch_all(&state.db)
    .await?;
    let mut visible = Vec::with_capacity(rows.len());
    for r in rows {
        if !access::is_trashed(&state.db, &r.node_id).await? {
            visible.push(r);
        }
    }
    Ok(Json(
        visible
            .into_iter()
            .map(|r| Link {
                id: r.id,
                token: r.token,
                node_id: r.node_id,
                has_password: r.has_password,
                expires_at: r.expires_at,
                created_at: r.created_at,
                upload_only: r.upload_only,
                max_opens: r.max_opens,
                opens: r.opens,
                enc_link_secret: r.enc_link_secret.map(B64),
            })
            .collect(),
    ))
}

pub async fn delete(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<String>,
) -> Result<StatusCode> {
    let owned: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM public_links WHERE id = ? AND owner_id = ?)",
    )
    .bind(&id)
    .bind(&user.id)
    .fetch_one(&state.db)
    .await?;
    if owned {
        crate::routes::uploads::discard_link_uploads(&state, std::slice::from_ref(&id)).await?;
    }
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
