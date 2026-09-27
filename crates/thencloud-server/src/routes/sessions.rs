//! Signed-in devices: list your sessions and sign them out.

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use thencloud_crypto::api::DeviceSession;

use crate::AppState;
use crate::auth::AuthUser;
use crate::error::{AppError, Result};
use crate::util::now;

pub async fn list(
    State(state): State<AppState>,
    user: AuthUser,
) -> Result<Json<Vec<DeviceSession>>> {
    let rows: Vec<(String, String, i64, i64, bool, Option<String>)> = sqlx::query_as(
        "SELECT s.id, s.device_name, s.created_at, s.last_seen, s.token_hash = ?, a.name FROM sessions s \
         LEFT JOIN app_passwords a ON a.id = s.app_password_id \
         WHERE s.user_id = ? AND s.expires_at > ? ORDER BY s.last_seen DESC",
    )
    .bind(&user.token_hash)
    .bind(&user.id)
    .bind(now())
    .fetch_all(&state.db)
    .await?;
    Ok(Json(
        rows.into_iter()
            .map(
                |(id, device_name, created_at, last_seen, current, app_password)| DeviceSession {
                    id,
                    device_name,
                    created_at,
                    last_seen,
                    current,
                    app_password,
                },
            )
            .collect(),
    ))
}

/// Sign out one of your sessions (your current one too, like logging out).
pub async fn revoke(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<String>,
) -> Result<StatusCode> {
    // A device signed in with an app password may only sign itself out.
    let own = if user.app_password_id.is_some() {
        Some(&user.token_hash)
    } else {
        None
    };
    let r = sqlx::query(
        "DELETE FROM sessions WHERE id = ? AND user_id = ? AND (? IS NULL OR token_hash = ?)",
    )
    .bind(&id)
    .bind(&user.id)
    .bind(own)
    .bind(own)
    .execute(&state.db)
    .await?;
    if r.rows_affected() == 0 {
        return Err(AppError::NotFound);
    }
    Ok(StatusCode::NO_CONTENT)
}

/// Sign out every session except this one.
pub async fn revoke_others(State(state): State<AppState>, user: AuthUser) -> Result<StatusCode> {
    if user.app_password_id.is_some() {
        return Err(AppError::Forbidden);
    }
    sqlx::query("DELETE FROM sessions WHERE user_id = ? AND token_hash != ?")
        .bind(&user.id)
        .bind(&user.token_hash)
        .execute(&state.db)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}
