//! Anonymous access through public links.
//!
//! Visitors only ever send the link token (from the URL path). The node key
//! stays in the URL fragment in the browser, so everything served here is
//! ciphertext the server itself cannot read.

use axum::Json;
use axum::body::Bytes;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::Response;
use thencloud_crypto::api::*;

use crate::AppState;
use crate::access;
use crate::auth::ClientIp;
use crate::db::{NodeRow, get_children, get_node};
use crate::error::{AppError, Result};
use crate::routes::nodes::current_chunk;
use crate::routes::uploads::{self, DropLink, Uploader};
use crate::util::*;

const LINK_SESSION_SECS: i64 = 12 * 3600;

#[derive(sqlx::FromRow)]
struct LinkRow {
    id: String,
    node_id: String,
    owner_id: String,
    password_hash: Option<String>,
    expires_at: Option<i64>,
    upload_only: bool,
}

async fn find(state: &AppState, token: &str) -> Result<LinkRow> {
    sqlx::query_as(
        "SELECT id, node_id, owner_id, password_hash, expires_at, upload_only FROM public_links \
         WHERE token = ? AND (expires_at IS NULL OR expires_at > ?)",
    )
    .bind(token)
    .bind(now())
    .fetch_optional(&state.db)
    .await?
    .ok_or(AppError::NotFound)
}

fn session_msg(token: &str, exp: i64) -> Vec<u8> {
    format!("link-session:{token}:{exp}").into_bytes()
}

/// Resolve a link and, if it is password protected, check the
/// `X-Link-Token` issued by [`unlock`].
async fn resolve(state: &AppState, token: &str, headers: &HeaderMap) -> Result<LinkRow> {
    let link = find(state, token).await?;
    if link.password_hash.is_some() {
        let ok = headers
            .get("x-link-token")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.split_once('.'))
            .and_then(|(exp, tag)| {
                Some((
                    exp.parse::<i64>().ok()?,
                    thencloud_crypto::b64_decode(tag).ok()?,
                ))
            })
            .is_some_and(|(exp, tag)| {
                exp > now() && hmac_verify(&state.secret[..], &session_msg(token, exp), &tag)
            });
        if !ok {
            return Err(AppError::PasswordRequired);
        }
    }
    Ok(link)
}

async fn node_in_link(state: &AppState, link: &LinkRow, node_id: &str) -> Result<NodeRow> {
    // Visitors to a file drop see nothing in the folder.
    if link.upload_only {
        return Err(AppError::Forbidden);
    }
    if !access::is_within(&state.db, node_id, &link.node_id).await?
        || access::is_trashed(&state.db, node_id).await?
    {
        return Err(AppError::NotFound);
    }
    get_node(&state.db, node_id)
        .await?
        .ok_or(AppError::NotFound)
}

pub async fn info(
    State(state): State<AppState>,
    Path(token): Path<String>,
    headers: HeaderMap,
) -> Result<Json<PublicLinkInfo>> {
    let link = resolve(&state, &token, &headers).await?;
    if access::is_trashed(&state.db, &link.node_id).await? {
        return Err(AppError::NotFound);
    }
    let node = get_node(&state.db, &link.node_id)
        .await?
        .ok_or(AppError::NotFound)?;
    let owner_pq_public_key = if link.upload_only {
        sqlx::query_scalar("SELECT pq_public_key FROM users WHERE id = ?")
            .bind(&link.owner_id)
            .fetch_one(&state.db)
            .await?
    } else {
        None
    };
    Ok(Json(PublicLinkInfo {
        owner_pq_public_key: owner_pq_public_key.map(B64),
        expires_at: link.expires_at,
        upload_only: link.upload_only,
        owner: link.upload_only.then(|| node.owner.clone()),
        folder_id: link.upload_only.then(|| node.id.clone()),
        node: (!link.upload_only).then(|| node.into_api()),
    }))
}

pub async fn unlock(
    State(state): State<AppState>,
    Path(token): Path<String>,
    ip: ClientIp,
    Json(req): Json<UnlockLinkRequest>,
) -> Result<Json<UnlockLinkResponse>> {
    let key = format!("link:{token}:{}", ip.key());
    if state.limiter.blocked(&key) {
        return Err(AppError::RateLimited);
    }
    let link = find(&state, &token).await?;
    let Some(hash) = link.password_hash else {
        return Err(AppError::bad("this link has no password"));
    };
    if !verify_secret(req.password.into_bytes(), hash).await? {
        state.limiter.fail(&key);
        return Err(AppError::InvalidCredentials);
    }
    state.limiter.clear(&key);
    let mut exp = now() + LINK_SESSION_SECS;
    if let Some(e) = link.expires_at {
        exp = exp.min(e);
    }
    let tag = hmac(&state.secret[..], &session_msg(&token, exp));
    Ok(Json(UnlockLinkResponse {
        link_token: format!("{exp}.{}", thencloud_crypto::b64_encode(&tag)),
        expires_at: exp,
    }))
}

pub async fn children(
    State(state): State<AppState>,
    Path((token, id)): Path<(String, String)>,
    headers: HeaderMap,
) -> Result<Json<Vec<Node>>> {
    let link = resolve(&state, &token, &headers).await?;
    let node = node_in_link(&state, &link, &id).await?;
    if !node.is_folder() {
        return Err(AppError::bad("not a folder"));
    }
    let kids = get_children(&state.db, &id).await?;
    Ok(Json(kids.into_iter().map(NodeRow::into_api).collect()))
}

pub async fn chunk(
    State(state): State<AppState>,
    Path((token, id, idx)): Path<(String, String, u32)>,
    headers: HeaderMap,
) -> Result<Response> {
    let link = resolve(&state, &token, &headers).await?;
    let node = node_in_link(&state, &link, &id).await?;
    current_chunk(&state, node, idx).await
}

/// Resolve an upload-only link for a visitor adding a file.
async fn drop_link(state: &AppState, token: &str, headers: &HeaderMap) -> Result<DropLink> {
    let link = resolve(state, token, headers).await?;
    if !link.upload_only {
        return Err(AppError::Forbidden);
    }
    Ok(DropLink {
        id: link.id,
        folder_id: link.node_id,
        owner_id: link.owner_id,
    })
}

pub async fn upload_create(
    State(state): State<AppState>,
    Path(token): Path<String>,
    headers: HeaderMap,
    Json(req): Json<CreateUploadRequest>,
) -> Result<(StatusCode, Json<UploadResponse>)> {
    if req.parent_id.is_none() {
        return Err(AppError::bad("a file drop only accepts new files"));
    }
    let link = drop_link(&state, &token, &headers).await?;
    uploads::start(&state, Uploader::Link(&link), req).await
}

pub async fn upload_chunk(
    State(state): State<AppState>,
    Path((token, id, idx)): Path<(String, String, u32)>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<StatusCode> {
    let link = drop_link(&state, &token, &headers).await?;
    uploads::store_chunk(&state, Uploader::Link(&link), &id, idx, body).await
}

/// Finishes the upload. The visitor learns nothing back but that it worked.
pub async fn upload_finish(
    State(state): State<AppState>,
    Path((token, id)): Path<(String, String)>,
    headers: HeaderMap,
) -> Result<StatusCode> {
    let link = drop_link(&state, &token, &headers).await?;
    let _ = uploads::publish(&state, Uploader::Link(&link), &id, Default::default()).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// A visitor gives up on an upload: its space is freed straight away.
pub async fn upload_abort(
    State(state): State<AppState>,
    Path((token, id)): Path<(String, String)>,
    headers: HeaderMap,
) -> Result<StatusCode> {
    let link = drop_link(&state, &token, &headers).await?;
    uploads::cancel(&state, Uploader::Link(&link), &id).await
}
