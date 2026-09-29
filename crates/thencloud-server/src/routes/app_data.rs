//! App data: the music and video libraries' playlists and edits, the
//! favourites and recent files, pinned notes, where books were left, the
//! index for searching inside files, the health log and which optional
//! modules are on (`prefs`), one blob per name,
//! encrypted under the user's master key. The server keeps it and a
//! revision number, and nothing else.

use axum::Json;
use axum::extract::{Path, State};
use thencloud_crypto::api::{B64, PrivateData, PutPrivateData};

use crate::AppState;
use crate::auth::AuthUser;
use crate::error::{AppError, Result};
use crate::util::now;

const NAMES: &[&str] = &[
    "music", "videos", "files", "notes", "books", "search", "health", "prefs",
];
const MAX_BYTES: usize = 2 * 1024 * 1024;
/// The search index (the words in each text file, for searching inside
/// them) is bigger than the libraries.
pub const MAX_SEARCH_BYTES: usize = 12 * 1024 * 1024;

fn check(name: &str) -> Result<()> {
    if NAMES.contains(&name) {
        Ok(())
    } else {
        Err(AppError::NotFound)
    }
}

pub async fn get(
    State(state): State<AppState>,
    user: AuthUser,
    Path(name): Path<String>,
) -> Result<Json<PrivateData>> {
    check(&name)?;
    let row: Option<(Vec<u8>, i64)> =
        sqlx::query_as("SELECT data, revision FROM app_data WHERE user_id = ? AND name = ?")
            .bind(&user.id)
            .bind(&name)
            .fetch_optional(&state.db)
            .await?;
    Ok(Json(match row {
        Some((data, revision)) => PrivateData {
            data: Some(B64(data)),
            revision,
        },
        None => PrivateData {
            data: None,
            revision: 0,
        },
    }))
}

pub async fn put(
    State(state): State<AppState>,
    user: AuthUser,
    Path(name): Path<String>,
    Json(req): Json<PutPrivateData>,
) -> Result<Json<PrivateData>> {
    check(&name)?;
    let max = if name == "search" {
        MAX_SEARCH_BYTES
    } else {
        MAX_BYTES
    };
    if req.data.0.len() > max {
        return Err(AppError::bad("the library data is too large"));
    }
    let r = if req.if_revision == 0 {
        sqlx::query(
            "INSERT INTO app_data (user_id, name, data, revision, updated_at) VALUES (?, ?, ?, 1, ?) \
             ON CONFLICT (user_id, name) DO NOTHING",
        )
        .bind(&user.id)
        .bind(&name)
        .bind(&req.data.0)
        .bind(now())
        .execute(&state.db)
        .await?
    } else {
        sqlx::query(
            "UPDATE app_data SET data = ?, revision = revision + 1, updated_at = ? \
             WHERE user_id = ? AND name = ? AND revision = ?",
        )
        .bind(&req.data.0)
        .bind(now())
        .bind(&user.id)
        .bind(&name)
        .bind(req.if_revision)
        .execute(&state.db)
        .await?
    };
    if r.rows_affected() == 0 {
        return Err(AppError::Conflict(
            "this changed on another device; reload and try again".into(),
        ));
    }
    Ok(Json(PrivateData {
        data: Some(req.data),
        revision: req.if_revision + 1,
    }))
}
