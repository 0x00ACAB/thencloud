//! End-to-end encrypted sharing between users.
//!
//! The owner seals the node key to the recipient's X25519 public key; the
//! server only stores and forwards that sealed blob.

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use serde::Deserialize;
use thencloud_crypto::api::*;

use crate::AppState;
use crate::access::{self, Access};
use crate::auth::AuthUser;
use crate::db::get_node;
use crate::error::{AppError, Result};
use crate::util::*;

fn perm_str(p: Permission) -> &'static str {
    match p {
        Permission::Read => "read",
        Permission::Write => "write",
    }
}

fn parse_perm(s: &str) -> Permission {
    if s == "write" {
        Permission::Write
    } else {
        Permission::Read
    }
}

pub async fn public_key(
    State(state): State<AppState>,
    _user: AuthUser,
    Path(username): Path<String>,
) -> Result<Json<UserPublicKey>> {
    let row: Option<(String, Vec<u8>, Option<Vec<u8>>)> =
        sqlx::query_as("SELECT username, public_key, pq_public_key FROM users WHERE username = ?")
            .bind(username.trim().to_lowercase())
            .fetch_optional(&state.db)
            .await?;
    let (username, pk, pq) = row.ok_or(AppError::NotFound)?;
    Ok(Json(UserPublicKey {
        username,
        public_key: B64(pk),
        pq_public_key: pq.map(B64),
    }))
}

/// Create a share, or update the key/permission of an existing one.
pub async fn create(
    State(state): State<AppState>,
    user: AuthUser,
    Json(req): Json<CreateShareRequest>,
) -> Result<(StatusCode, Json<OutgoingShare>)> {
    check_id(&req.node_id, "node_id")?;
    check_sealed(&req.wrapped_key, "wrapped_key")?;
    access::require(&state.db, &user.id, &req.node_id, Access::Owner).await?;
    let recipient: Option<(String, String)> =
        sqlx::query_as("SELECT id, username FROM users WHERE username = ?")
            .bind(req.recipient.trim().to_lowercase())
            .fetch_optional(&state.db)
            .await?;
    let (recipient_id, recipient_name) = recipient.ok_or(AppError::NotFound)?;
    if recipient_id == user.id {
        return Err(AppError::bad("you cannot share with yourself"));
    }
    if req.expires_at.is_some_and(|e| e <= now()) {
        return Err(AppError::bad("expires_at must be in the future"));
    }
    // Sharing again replaces the key, permission and expiry. A share that
    // has expired but not been cleaned up yet starts again from now.
    sqlx::query(
        "INSERT INTO shares (id, node_id, owner_id, recipient_id, wrapped_key, permission, created_at, expires_at) \
         VALUES (?, ?, ?, ?, ?, ?, ?, ?) \
         ON CONFLICT (node_id, recipient_id) DO UPDATE SET wrapped_key = excluded.wrapped_key, \
         permission = excluded.permission, expires_at = excluded.expires_at, \
         created_at = CASE WHEN shares.expires_at <= excluded.created_at THEN excluded.created_at ELSE shares.created_at END",
    )
    .bind(new_uuid())
    .bind(&req.node_id)
    .bind(&user.id)
    .bind(&recipient_id)
    .bind(&req.wrapped_key.0)
    .bind(perm_str(req.permission))
    .bind(now())
    .bind(req.expires_at)
    .execute(&state.db)
    .await?;
    let (id, created_at): (String, i64) =
        sqlx::query_as("SELECT id, created_at FROM shares WHERE node_id = ? AND recipient_id = ?")
            .bind(&req.node_id)
            .bind(&recipient_id)
            .fetch_one(&state.db)
            .await?;
    Ok((
        StatusCode::CREATED,
        Json(OutgoingShare {
            id,
            recipient: recipient_name,
            permission: req.permission,
            node_id: req.node_id,
            created_at,
            expires_at: req.expires_at,
        }),
    ))
}

#[derive(sqlx::FromRow)]
struct IncomingRow {
    id: String,
    owner: String,
    owner_pk: Vec<u8>,
    owner_pq: Option<Vec<u8>>,
    permission: String,
    wrapped_key: Vec<u8>,
    node_id: String,
    created_at: i64,
    expires_at: Option<i64>,
}

pub async fn incoming(
    State(state): State<AppState>,
    user: AuthUser,
) -> Result<Json<Vec<IncomingShare>>> {
    let rows: Vec<IncomingRow> = sqlx::query_as(
        "SELECT s.id, o.username AS owner, o.public_key AS owner_pk, o.pq_public_key AS owner_pq, s.permission, s.wrapped_key, s.node_id, s.created_at, s.expires_at \
         FROM shares s JOIN users o ON o.id = s.owner_id WHERE s.recipient_id = ? AND (s.expires_at IS NULL OR s.expires_at > ?) \
         ORDER BY s.created_at",
    )
    .bind(&user.id)
    .bind(now())
    .fetch_all(&state.db)
    .await?;
    let mut out = Vec::with_capacity(rows.len());
    for r in rows {
        if access::is_trashed(&state.db, &r.node_id).await? {
            continue;
        }
        let Some(node) = get_node(&state.db, &r.node_id).await? else {
            continue;
        };
        out.push(IncomingShare {
            id: r.id,
            owner: r.owner,
            owner_public_key: B64(r.owner_pk),
            owner_pq_public_key: r.owner_pq.map(B64),
            permission: parse_perm(&r.permission),
            wrapped_key: B64(r.wrapped_key),
            node: node.into_api(),
            created_at: r.created_at,
            expires_at: r.expires_at,
        });
    }
    Ok(Json(out))
}

#[derive(Deserialize)]
pub struct NodeFilter {
    pub node_id: Option<String>,
}

pub async fn outgoing(
    State(state): State<AppState>,
    user: AuthUser,
    Query(f): Query<NodeFilter>,
) -> Result<Json<Vec<OutgoingShare>>> {
    let rows: Vec<(String, String, String, String, i64, Option<i64>)> = sqlx::query_as(
        "SELECT s.id, r.username, s.permission, s.node_id, s.created_at, s.expires_at FROM shares s \
         JOIN users r ON r.id = s.recipient_id WHERE s.owner_id = ? AND (? IS NULL OR s.node_id = ?) \
         AND (s.expires_at IS NULL OR s.expires_at > ?) ORDER BY s.created_at",
    )
    .bind(&user.id)
    .bind(&f.node_id)
    .bind(&f.node_id)
    .bind(now())
    .fetch_all(&state.db)
    .await?;
    let mut visible = Vec::with_capacity(rows.len());
    for r in rows {
        if !access::is_trashed(&state.db, &r.3).await? {
            visible.push(r);
        }
    }
    Ok(Json(
        visible
            .into_iter()
            .map(
                |(id, recipient, perm, node_id, created_at, expires_at)| OutgoingShare {
                    id,
                    recipient,
                    permission: parse_perm(&perm),
                    node_id,
                    created_at,
                    expires_at,
                },
            )
            .collect(),
    ))
}

pub async fn update(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<String>,
    Json(req): Json<UpdateShareRequest>,
) -> Result<StatusCode> {
    let res = sqlx::query("UPDATE shares SET permission = ? WHERE id = ? AND owner_id = ?")
        .bind(perm_str(req.permission))
        .bind(&id)
        .bind(&user.id)
        .execute(&state.db)
        .await?;
    if res.rows_affected() == 0 {
        return Err(AppError::NotFound);
    }
    Ok(StatusCode::NO_CONTENT)
}

/// The recipient seals a share's node key again, to their own hybrid
/// X25519 + ML-KEM-768 key: shares made before they had an ML-KEM key were
/// sealed to X25519 alone, which a future quantum computer could open from
/// a recording. Only the recipient can do it (they can open the old box),
/// only to a hybrid box, and only once they have an ML-KEM key. The server
/// can't check what's inside; a recipient could only lock themselves out.
pub async fn reseal(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<String>,
    Json(req): Json<ResealShareRequest>,
) -> Result<StatusCode> {
    if req.wrapped_key.len() != HYBRID_SEALED_KEY_LEN {
        return Err(AppError::bad("wrapped_key must be a hybrid sealed box"));
    }
    let has_pq: Option<bool> =
        sqlx::query_scalar("SELECT pq_public_key IS NOT NULL FROM users WHERE id = ?")
            .bind(&user.id)
            .fetch_optional(&state.db)
            .await?;
    if has_pq != Some(true) {
        return Err(AppError::bad("you have no ML-KEM key to seal to"));
    }
    let res = sqlx::query("UPDATE shares SET wrapped_key = ? WHERE id = ? AND recipient_id = ?")
        .bind(&req.wrapped_key.0)
        .bind(&id)
        .bind(&user.id)
        .execute(&state.db)
        .await?;
    if res.rows_affected() == 0 {
        return Err(AppError::NotFound);
    }
    Ok(StatusCode::NO_CONTENT)
}

/// Revoke (owner) or leave (recipient) a share.
///
/// Note: revocation stops the server from serving the data, but a former
/// recipient who kept the node key could still decrypt ciphertext obtained
/// elsewhere. Re-keying on revocation is planned (see MILESTONES.md).
pub async fn delete(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<String>,
) -> Result<StatusCode> {
    let pair: Option<(String, String)> = sqlx::query_as(
        "SELECT owner_id, recipient_id FROM shares WHERE id = ? AND (owner_id = ? OR recipient_id = ?)",
    )
    .bind(&id)
    .bind(&user.id)
    .bind(&user.id)
    .fetch_optional(&state.db)
    .await?;
    let (owner, recipient) = pair.ok_or(AppError::NotFound)?;
    sqlx::query("DELETE FROM shares WHERE id = ?")
        .bind(&id)
        .execute(&state.db)
        .await?;
    crate::routes::avatars::drop_unrelated_grants(&state, &owner, &recipient).await?;
    Ok(StatusCode::NO_CONTENT)
}
