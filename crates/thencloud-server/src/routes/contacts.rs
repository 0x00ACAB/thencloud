//! Verified contacts, stored as one blob encrypted under the user's master
//! key. The server keeps it and a revision number, and nothing else.

use axum::Json;
use axum::extract::State;
use thencloud_crypto::api::{B64, PrivateData, PutPrivateData};

use crate::AppState;
use crate::auth::AuthUser;
use crate::error::{AppError, Result};

/// Plenty for thousands of contacts.
const MAX_CONTACTS_BYTES: usize = 256 * 1024;

pub async fn get(State(state): State<AppState>, user: AuthUser) -> Result<Json<PrivateData>> {
    let (data, revision): (Option<Vec<u8>>, i64) =
        sqlx::query_as("SELECT enc_contacts, contacts_revision FROM users WHERE id = ?")
            .bind(&user.id)
            .fetch_one(&state.db)
            .await?;
    Ok(Json(PrivateData {
        data: data.map(B64),
        revision,
    }))
}

pub async fn put(
    State(state): State<AppState>,
    user: AuthUser,
    Json(req): Json<PutPrivateData>,
) -> Result<Json<PrivateData>> {
    if req.data.0.len() > MAX_CONTACTS_BYTES {
        return Err(AppError::bad("contacts are too large"));
    }
    let r = sqlx::query(
        "UPDATE users SET enc_contacts = ?, contacts_revision = contacts_revision + 1 \
         WHERE id = ? AND contacts_revision = ?",
    )
    .bind(&req.data.0)
    .bind(&user.id)
    .bind(req.if_revision)
    .execute(&state.db)
    .await?;
    if r.rows_affected() == 0 {
        return Err(AppError::Conflict(
            "contacts changed on another device; reload and try again".into(),
        ));
    }
    Ok(Json(PrivateData {
        data: Some(req.data),
        revision: req.if_revision + 1,
    }))
}
