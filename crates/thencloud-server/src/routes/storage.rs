//! Linked storage accounts (see storage/mod.rs): list them, link Google
//! Drive through its consent page, change what one is for, move files to
//! or from it, unlink it, and say where new files go first.
//!
//! Linking happens in a popup the web client opens on Google's page, so
//! the app (and the keys it holds in memory) stays where it is. Google sends
//! the popup back to `/api/storage/google/callback`, which finishes the link
//! and sends it on to `/storage-linked.html`; that page tells the app and
//! closes.

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Redirect, Response};
use serde::Deserialize;
use thencloud_crypto::api::*;

use crate::AppState;
use crate::auth::AuthUser;
use crate::error::{AppError, Result};
use crate::routes::two_factor::{drop_challenge, get_challenge, put_challenge};
use crate::storage::{self, Account};
use crate::util::*;

/// Linked accounts one person may have.
const MAX_ACCOUNTS: i64 = 5;
const CALLBACK: &str = "/api/storage/google/callback";

fn mode_str(m: StorageMode) -> &'static str {
    match m {
        StorageMode::Mirror => "mirror",
        StorageMode::Extra => "extra",
    }
}

fn parse_mode(s: &str) -> StorageMode {
    if s == "extra" {
        StorageMode::Extra
    } else {
        StorageMode::Mirror
    }
}

pub async fn info(State(state): State<AppState>, user: AuthUser) -> Result<Json<StorageInfo>> {
    let (prefer, used, quota): (String, i64, i64) =
        sqlx::query_as("SELECT storage_prefer, used_bytes, quota_bytes FROM users WHERE id = ?")
            .bind(&user.id)
            .fetch_one(&state.db)
            .await?;
    let mut accounts = Vec::new();
    for a in storage::accounts_of(&state.db, &user.id).await? {
        let (mirror_total, mirror_done) = if a.mode == "mirror" {
            let (t, d) = storage::mirror_progress(&state.db, &a).await?;
            (Some(t), Some(d))
        } else {
            (None, None)
        };
        let only_there = storage::only_there(&state.db, &a.id).await?;
        accounts.push(StorageAccount {
            label: a
                .enc_label
                .as_deref()
                .and_then(|l| state.storage.open(&a.id, "label", l)),
            id: a.id,
            provider: a.provider,
            mode: parse_mode(&a.mode),
            used_bytes: a.used_bytes,
            free_bytes: a.free_bytes,
            broken: a.broken_at.is_some(),
            created_at: a.created_at,
            mirror_total,
            mirror_done,
            only_there,
        });
    }
    Ok(Json(StorageInfo {
        google: state.storage.google.is_some(),
        prefer: if prefer == "linked" {
            StoragePrefer::Linked
        } else {
            StoragePrefer::Server
        },
        server_used: used,
        server_quota: quota,
        accounts,
    }))
}

/// Where Google sends the browser back to: the address people use, or
/// failing that the one this request came to.
fn redirect_uri(state: &AppState, headers: &HeaderMap) -> Result<String> {
    if let Some(o) = state.config.public_origin.first() {
        return Ok(format!("{}{CALLBACK}", o.trim_end_matches('/')));
    }
    let host = headers
        .get(header::HOST)
        .and_then(|v| v.to_str().ok())
        .filter(|h| !h.is_empty() && !h.contains(['/', '@', ' ']))
        .ok_or_else(|| AppError::bad("no Host header"))?;
    let local = {
        let name = host.rsplit_once(':').map_or(host, |(h, _)| h);
        name == "localhost" || name == "127.0.0.1" || name == "[::1]"
    };
    let forwarded = state
        .config
        .trust_proxy
        .then(|| {
            headers
                .get("x-forwarded-proto")
                .and_then(|v| v.to_str().ok())
        })
        .flatten();
    let scheme = match forwarded {
        Some("http") => "http",
        Some(_) => "https",
        None if local => "http",
        None => "https",
    };
    Ok(format!("{scheme}://{host}{CALLBACK}"))
}

/// Start linking Google Drive: the consent page to open.
pub async fn google_start(
    State(state): State<AppState>,
    user: AuthUser,
    headers: HeaderMap,
    Json(req): Json<LinkStorageRequest>,
) -> Result<Json<LinkStorageResponse>> {
    if user.app_password_id.is_some() {
        return Err(AppError::Forbidden);
    }
    let g = state
        .storage
        .google
        .as_ref()
        .ok_or_else(|| AppError::Unavailable("this server can't link Google Drive".into()))?;
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM storage_accounts WHERE user_id = ?")
        .bind(&user.id)
        .fetch_one(&state.db)
        .await?;
    if count >= MAX_ACCOUNTS {
        return Err(AppError::bad(format!(
            "you can link at most {MAX_ACCOUNTS} storage accounts"
        )));
    }
    let redirect = redirect_uri(&state, &headers)?;
    // The ticket names the person, so the callback (a plain navigation,
    // without their token) knows whose account it is. Single use, 5 minutes.
    let data = format!("{}\n{redirect}", mode_str(req.mode));
    let ticket = put_challenge(
        &state,
        Some(&user.id),
        "storage-link",
        &thencloud_crypto::random_bytes(16),
        Some(data.as_bytes()),
    )
    .await?;
    Ok(Json(LinkStorageResponse {
        url: g.auth_url(&redirect, &ticket),
    }))
}

#[derive(Deserialize)]
pub struct CallbackQuery {
    code: Option<String>,
    state: Option<String>,
    error: Option<String>,
}

/// Google's redirect back. Always ends on `/storage-linked.html`, saying how
/// it went after the `#`.
pub async fn google_callback(
    State(state): State<AppState>,
    Query(q): Query<CallbackQuery>,
) -> Response {
    let outcome = match finish_link(&state, &q).await {
        Ok(()) => "linked",
        Err(Outcome::Denied) => "denied",
        Err(Outcome::Expired) => "expired",
        Err(Outcome::Failed) => "failed",
    };
    Redirect::to(&format!("/storage-linked.html#{outcome}")).into_response()
}

enum Outcome {
    Denied,
    Expired,
    Failed,
}

async fn finish_link(state: &AppState, q: &CallbackQuery) -> std::result::Result<(), Outcome> {
    let ticket = q.state.as_deref().ok_or(Outcome::Expired)?;
    let ch = get_challenge(state, ticket, "storage-link")
        .await
        .map_err(|_| Outcome::Expired)?;
    let _ = drop_challenge(state, ticket).await;
    let user_id = ch.user_id.ok_or(Outcome::Expired)?;
    if q.error.is_some() {
        return Err(Outcome::Denied);
    }
    let code = q.code.as_deref().ok_or(Outcome::Denied)?;
    let data = String::from_utf8(ch.data.unwrap_or_default()).map_err(|_| Outcome::Failed)?;
    let (mode, redirect) = data.split_once('\n').ok_or(Outcome::Failed)?;
    let g = state.storage.google.as_ref().ok_or(Outcome::Failed)?;
    let fail = |e: storage::google::DriveError| {
        tracing::warn!(error = %e, "linking Google Drive failed");
        Outcome::Failed
    };
    let tokens = g.exchange(code, redirect).await.map_err(fail)?;
    let about = g.about(&tokens.access).await.map_err(fail)?;
    let folder = g.create_folder(&tokens.access).await.map_err(fail)?;
    let id = new_uuid();
    sqlx::query(
        "INSERT INTO storage_accounts (id, user_id, provider, mode, enc_token, enc_label, folder_id, \
         free_bytes, checked_at, created_at) VALUES (?, ?, 'google', ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&id)
    .bind(&user_id)
    .bind(if mode == "extra" { "extra" } else { "mirror" })
    .bind(state.storage.seal(&id, "refresh-token", &tokens.refresh))
    .bind(state.storage.seal(&id, "label", &about.email))
    .bind(&folder)
    .bind(about.free)
    .bind(now())
    .bind(now())
    .execute(&state.db)
    .await
    .map_err(|_| Outcome::Failed)?;
    g.keep_access(&id, &tokens.access, tokens.expires_in);
    Ok(())
}

async fn own_account(state: &AppState, user: &AuthUser, id: &str) -> Result<Account> {
    storage::account(&state.db, id)
        .await?
        .filter(|a| a.user_id == user.id)
        .ok_or(AppError::NotFound)
}

pub async fn update_account(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<String>,
    Json(req): Json<UpdateStorageAccountRequest>,
) -> Result<StatusCode> {
    let a = own_account(&state, &user, &id).await?;
    sqlx::query("UPDATE storage_accounts SET mode = ? WHERE id = ?")
        .bind(mode_str(req.mode))
        .bind(&a.id)
        .execute(&state.db)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn unlink(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<String>,
) -> Result<StatusCode> {
    if user.app_password_id.is_some() {
        return Err(AppError::Forbidden);
    }
    let a = own_account(&state, &user, &id).await?;
    storage::unlink(&state, &a).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// Move a batch of files between this server and the account.
pub async fn move_files(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<String>,
    Json(req): Json<MoveStorageRequest>,
) -> Result<Json<MoveStorageResponse>> {
    let a = own_account(&state, &user, &id).await?;
    let m = storage::move_batch(&state, &a, req.to == StoragePrefer::Server, None).await?;
    Ok(Json(MoveStorageResponse {
        moved: m.versions,
        moved_bytes: m.bytes,
        left: m.left,
        left_bytes: m.left_bytes,
        full: m.full,
    }))
}

/// The node, if `user` owns it (trashed or not: its versions still take
/// room somewhere).
async fn own_node(state: &AppState, user: &AuthUser, id: &str) -> Result<()> {
    let owner: Option<String> = sqlx::query_scalar("SELECT owner_id FROM nodes WHERE id = ?")
        .bind(id)
        .fetch_optional(&state.db)
        .await?;
    match owner {
        Some(o) if o == user.id => Ok(()),
        _ => Err(AppError::NotFound),
    }
}

/// Where a file's (or a folder's files') versions are kept.
pub async fn node_places(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<String>,
) -> Result<Json<NodeStorage>> {
    own_node(&state, &user, &id).await?;
    let rows: Vec<(Option<String>, i64, i64)> = sqlx::query_as(
        "WITH RECURSIVE sub(id) AS (SELECT ? UNION ALL \
           SELECT n.id FROM nodes n JOIN sub ON n.parent_id = sub.id) \
         SELECT v.account_id, COUNT(*), COALESCE(SUM(v.size), 0) FROM file_versions v \
         WHERE v.node_id IN (SELECT id FROM sub) GROUP BY v.account_id ORDER BY v.account_id",
    )
    .bind(&id)
    .fetch_all(&state.db)
    .await?;
    Ok(Json(NodeStorage {
        places: rows
            .into_iter()
            .map(|(account_id, versions, bytes)| NodePlace {
                account_id,
                versions,
                bytes,
            })
            .collect(),
    }))
}

/// Move a batch of a file's (or a folder's files') versions.
pub async fn move_node(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<String>,
    Json(req): Json<MoveNodeStorageRequest>,
) -> Result<Json<MoveStorageResponse>> {
    own_node(&state, &user, &id).await?;
    let m = match req.to {
        Some(account) => {
            let a = own_account(&state, &user, &account).await?;
            storage::move_batch(&state, &a, false, Some(&id)).await?
        }
        // Home from every account that has some of them.
        None => {
            let mut total = storage::Moved::default();
            for a in storage::accounts_of(&state.db, &user.id).await? {
                if a.broken_at.is_some() {
                    continue;
                }
                let m = storage::move_batch(&state, &a, true, Some(&id)).await?;
                total.versions += m.versions;
                total.bytes += m.bytes;
                total.left += m.left;
                total.left_bytes += m.left_bytes;
                total.full |= m.full;
            }
            total
        }
    };
    Ok(Json(MoveStorageResponse {
        moved: m.versions,
        moved_bytes: m.bytes,
        left: m.left,
        left_bytes: m.left_bytes,
        full: m.full,
    }))
}

pub async fn set_prefer(
    State(state): State<AppState>,
    user: AuthUser,
    Json(req): Json<StoragePreferRequest>,
) -> Result<StatusCode> {
    sqlx::query("UPDATE users SET storage_prefer = ? WHERE id = ?")
        .bind(match req.prefer {
            StoragePrefer::Server => "server",
            StoragePrefer::Linked => "linked",
        })
        .bind(&user.id)
        .execute(&state.db)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}
