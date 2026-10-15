//! Signed-in devices: list your sessions and sign them out.

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use thencloud_crypto::api::DeviceSession;

use crate::AppState;
use crate::auth::AuthUser;
use crate::error::{AppError, Result};
use crate::util::now;

#[derive(sqlx::FromRow)]
struct SessionRow {
    id: String,
    device_name: String,
    created_at: i64,
    last_seen: i64,
    current: bool,
    app_password: Option<String>,
    method: Option<String>,
}

pub async fn list(
    State(state): State<AppState>,
    user: AuthUser,
) -> Result<Json<Vec<DeviceSession>>> {
    let rows: Vec<SessionRow> = sqlx::query_as(
        "SELECT s.id, s.device_name, s.created_at, s.last_seen, s.token_hash = ? AS current, \
         a.name AS app_password, s.method FROM sessions s \
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
            .map(|r| DeviceSession {
                id: r.id,
                device_name: r.device_name,
                created_at: r.created_at,
                last_seen: r.last_seen,
                current: r.current,
                app_password: r.app_password,
                method: r.method,
            })
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
