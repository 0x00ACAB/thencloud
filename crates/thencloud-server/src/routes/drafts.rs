//! Unsaved edits to text files, kept so a closed tab or a crash doesn't lose
//! them. Each is encrypted under its author's master key and bound to them
//! and the file; only someone who can write the file can keep one.

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use thencloud_crypto::api::{B64, Draft};

use crate::AppState;
use crate::access::{self, Access};
use crate::auth::AuthUser;
use crate::error::{AppError, Result};
use crate::util::now;

/// Text files are edited up to 5 MiB; this leaves room for the encryption.
const MAX_DRAFT_BYTES: usize = 5 * 1024 * 1024 + 1024;
/// All of one user's drafts together.
const MAX_TOTAL_BYTES: i64 = 32 * 1024 * 1024;

pub async fn get(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<String>,
) -> Result<Json<Option<Draft>>> {
    access::require(&state.db, &user.id, &id, Access::Write).await?;
    let row: Option<(Vec<u8>, i64, i64)> = sqlx::query_as(
        "SELECT data, base_revision, updated_at FROM drafts WHERE node_id = ? AND user_id = ?",
    )
    .bind(&id)
    .bind(&user.id)
    .fetch_optional(&state.db)
    .await?;
    Ok(Json(row.map(|(data, base_revision, updated_at)| Draft {
        data: B64(data),
        base_revision,
        updated_at,
    })))
}

pub async fn put(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<String>,
    Json(req): Json<Draft>,
) -> Result<StatusCode> {
    access::require(&state.db, &user.id, &id, Access::Write).await?;
    if req.data.0.len() > MAX_DRAFT_BYTES {
        return Err(AppError::bad("the draft is too large"));
    }
    let others: i64 = sqlx::query_scalar(
        "SELECT COALESCE(SUM(LENGTH(data)), 0) FROM drafts WHERE user_id = ? AND node_id != ?",
    )
    .bind(&user.id)
    .bind(&id)
    .fetch_one(&state.db)
    .await?;
    if others + req.data.0.len() as i64 > MAX_TOTAL_BYTES {
        return Err(AppError::bad(
            "too many unsaved drafts; save or discard some first",
        ));
    }
    sqlx::query(
        "INSERT INTO drafts (node_id, user_id, data, base_revision, updated_at) VALUES (?, ?, ?, ?, ?) \
         ON CONFLICT (node_id, user_id) DO UPDATE SET data = excluded.data, \
         base_revision = excluded.base_revision, updated_at = excluded.updated_at",
    )
    .bind(&id)
    .bind(&user.id)
    .bind(&req.data.0)
    .bind(req.base_revision)
    .bind(now())
    .execute(&state.db)
    .await?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn delete(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<String>,
) -> Result<StatusCode> {
    sqlx::query("DELETE FROM drafts WHERE node_id = ? AND user_id = ?")
        .bind(&id)
        .bind(&user.id)
        .execute(&state.db)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}
