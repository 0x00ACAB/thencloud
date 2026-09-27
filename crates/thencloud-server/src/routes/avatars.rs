//! Profile pictures. Each is encrypted under its owner's avatar key, which
//! the owner seals to the people they share with (either way round); the
//! server stores ciphertext and sealed keys and can't see the pictures.

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use thencloud_crypto::api::{AvatarGrant, B64, MyAvatar, SetAvatar, UserAvatar};

use crate::AppState;
use crate::auth::AuthUser;
use crate::error::{AppError, Result};
use crate::util::*;

/// A 256 x 256 picture is well under this.
const MAX_AVATAR_BYTES: usize = 256 * 1024;

pub async fn get_mine(State(state): State<AppState>, user: AuthUser) -> Result<Json<MyAvatar>> {
    let (data, enc_key): (Option<Vec<u8>>, Option<Vec<u8>>) =
        sqlx::query_as("SELECT enc_avatar, enc_avatar_key FROM users WHERE id = ?")
            .bind(&user.id)
            .fetch_one(&state.db)
            .await?;
    let grantees: Vec<String> = sqlx::query_scalar(
        "SELECT u.username FROM avatar_grants g JOIN users u ON u.id = g.grantee_id \
         WHERE g.owner_id = ? ORDER BY u.username",
    )
    .bind(&user.id)
    .fetch_all(&state.db)
    .await?;
    Ok(Json(MyAvatar {
        data: data.map(B64),
        enc_key: enc_key.map(B64),
        grantees,
    }))
}

pub async fn set(
    State(state): State<AppState>,
    user: AuthUser,
    Json(req): Json<SetAvatar>,
) -> Result<StatusCode> {
    if req.data.0.len() > MAX_AVATAR_BYTES {
        return Err(AppError::bad("the picture is too large"));
    }
    check_len(&req.enc_key, WRAPPED_KEY_LEN, "enc_key")?;
    sqlx::query(
        "UPDATE users SET enc_avatar = ?, enc_avatar_key = ?, avatar_updated_at = ? WHERE id = ?",
    )
    .bind(&req.data.0)
    .bind(&req.enc_key.0)
    .bind(now())
    .bind(&user.id)
    .execute(&state.db)
    .await?;
    Ok(StatusCode::NO_CONTENT)
}

/// Remove the picture and every copy of its key handed out.
pub async fn remove(State(state): State<AppState>, user: AuthUser) -> Result<StatusCode> {
    let mut tx = state.db.begin().await?;
    sqlx::query(
        "UPDATE users SET enc_avatar = NULL, enc_avatar_key = NULL, avatar_updated_at = NULL WHERE id = ?",
    )
    .bind(&user.id)
    .execute(&mut *tx)
    .await?;
    sqlx::query("DELETE FROM avatar_grants WHERE owner_id = ?")
        .bind(&user.id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn user_id(state: &AppState, username: &str) -> Result<String> {
    sqlx::query_scalar("SELECT id FROM users WHERE username = ?")
        .bind(username.trim().to_lowercase())
        .fetch_optional(&state.db)
        .await?
        .ok_or(AppError::NotFound)
}

/// Give `username` the avatar key. Only between people who share something
/// with each other.
pub async fn grant(
    State(state): State<AppState>,
    user: AuthUser,
    Path(username): Path<String>,
    Json(req): Json<AvatarGrant>,
) -> Result<StatusCode> {
    check_len(&req.sealed_key, SEALED_KEY_LEN, "sealed_key")?;
    let other = user_id(&state, &username).await?;
    let related: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM shares WHERE (owner_id = ? AND recipient_id = ?) \
         OR (owner_id = ? AND recipient_id = ?))",
    )
    .bind(&user.id)
    .bind(&other)
    .bind(&other)
    .bind(&user.id)
    .fetch_one(&state.db)
    .await?;
    if !related {
        return Err(AppError::Forbidden);
    }
    sqlx::query(
        "INSERT INTO avatar_grants (owner_id, grantee_id, sealed_key, created_at) VALUES (?, ?, ?, ?) \
         ON CONFLICT (owner_id, grantee_id) DO UPDATE SET sealed_key = excluded.sealed_key",
    )
    .bind(&user.id)
    .bind(&other)
    .bind(&req.sealed_key.0)
    .bind(now())
    .execute(&state.db)
    .await?;
    Ok(StatusCode::NO_CONTENT)
}

/// Someone's picture, if they gave the caller their avatar key.
pub async fn get_user(
    State(state): State<AppState>,
    user: AuthUser,
    Path(username): Path<String>,
) -> Result<Json<Option<UserAvatar>>> {
    let row: Option<(Vec<u8>, Vec<u8>, i64)> = sqlx::query_as(
        "SELECT u.enc_avatar, g.sealed_key, u.avatar_updated_at FROM users u \
         JOIN avatar_grants g ON g.owner_id = u.id AND g.grantee_id = ? \
         WHERE u.username = ? AND u.enc_avatar IS NOT NULL",
    )
    .bind(&user.id)
    .bind(username.trim().to_lowercase())
    .fetch_optional(&state.db)
    .await?;
    Ok(Json(row.map(|(data, sealed_key, updated_at)| UserAvatar {
        data: B64(data),
        sealed_key: B64(sealed_key),
        updated_at,
    })))
}
