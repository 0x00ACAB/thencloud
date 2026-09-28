//! Encrypted thumbnails. The browser makes one when it uploads an image and
//! seals it under the file's key, bound to the version; the server keeps it
//! next to that version and serves the current one to whoever can read the
//! file (or reach it through a public link).

use axum::body::Bytes;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};

use crate::AppState;
use crate::access::{self, Access};
use crate::auth::AuthUser;
use crate::error::{AppError, Result};

/// A thumbnail is a small JPEG; this leaves room for the encryption.
pub const MAX_THUMBNAIL_BYTES: usize = 64 * 1024;

pub async fn put(
    State(state): State<AppState>,
    user: AuthUser,
    Path((id, vid)): Path<(String, String)>,
    body: Bytes,
) -> Result<StatusCode> {
    access::require(&state.db, &user.id, &id, Access::Write).await?;
    if body.len() < 40 || body.len() > MAX_THUMBNAIL_BYTES {
        return Err(AppError::bad("a thumbnail must be at most 64 KiB"));
    }
    let res = sqlx::query("UPDATE file_versions SET thumbnail = ? WHERE id = ? AND node_id = ?")
        .bind(&body[..])
        .bind(&vid)
        .bind(&id)
        .execute(&state.db)
        .await?;
    if res.rows_affected() == 0 {
        return Err(AppError::NotFound);
    }
    Ok(StatusCode::NO_CONTENT)
}

pub async fn get(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<String>,
) -> Result<Response> {
    access::require(&state.db, &user.id, &id, Access::Read).await?;
    current(&state, &id).await
}

/// The current version's thumbnail, with its version id in `X-Version-Id`
/// (it's bound to that version).
pub async fn current(state: &AppState, node_id: &str) -> Result<Response> {
    let row: Option<(String, Option<Vec<u8>>)> = sqlx::query_as(
        "SELECT v.id, v.thumbnail FROM nodes n JOIN file_versions v ON v.id = n.current_version_id \
         WHERE n.id = ?",
    )
    .bind(node_id)
    .fetch_optional(&state.db)
    .await?;
    let Some((vid, Some(thumb))) = row else {
        return Err(AppError::NotFound);
    };
    let mut h = HeaderMap::new();
    h.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/octet-stream"),
    );
    h.insert(
        "x-version-id",
        HeaderValue::from_str(&vid).map_err(|e| AppError::Internal(e.to_string()))?,
    );
    Ok((h, thumb).into_response())
}
