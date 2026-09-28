//! Comments on files and folders. Each is encrypted under the node key in
//! the browser, so everyone who can open the node can read its comments and
//! the server can't. Anyone who can read a node can comment on it; a
//! comment can be deleted by its author or the node's owner.

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use thencloud_crypto::api::{B64, Comment, CreateCommentRequest};

use crate::AppState;
use crate::access::{self, Access};
use crate::auth::AuthUser;
use crate::error::{AppError, Result};
use crate::util::{check_id, coarse_now};

/// A few pages of text, with room for the encryption.
const MAX_BODY_BYTES: usize = 16 * 1024;
/// So one node can't grow without bound.
const MAX_PER_NODE: i64 = 5000;

#[derive(sqlx::FromRow)]
struct Row {
    id: String,
    node_id: String,
    author_id: String,
    author: String,
    created_at: i64,
    enc_body: Vec<u8>,
}

pub async fn list(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<String>,
) -> Result<Json<Vec<Comment>>> {
    access::require(&state.db, &user.id, &id, Access::Read).await?;
    let rows: Vec<Row> = sqlx::query_as(
        "SELECT c.id, c.node_id, c.author_id, u.username AS author, c.created_at, c.enc_body \
         FROM comments c JOIN users u ON u.id = c.author_id \
         WHERE c.node_id = ? ORDER BY c.created_at, c.rowid",
    )
    .bind(&id)
    .fetch_all(&state.db)
    .await?;
    Ok(Json(
        rows.into_iter()
            .map(|r| Comment {
                id: r.id,
                node_id: r.node_id,
                author_id: r.author_id,
                author: r.author,
                created_at: r.created_at,
                enc_body: B64(r.enc_body),
            })
            .collect(),
    ))
}

pub async fn create(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<String>,
    Json(req): Json<CreateCommentRequest>,
) -> Result<(StatusCode, Json<Comment>)> {
    check_id(&req.id, "id")?;
    access::require(&state.db, &user.id, &id, Access::Read).await?;
    if req.enc_body.0.is_empty() || req.enc_body.0.len() > MAX_BODY_BYTES {
        return Err(AppError::bad("a comment must be between 1 byte and 16 KiB"));
    }
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM comments WHERE node_id = ?")
        .bind(&id)
        .fetch_one(&state.db)
        .await?;
    if count >= MAX_PER_NODE {
        return Err(AppError::bad("this has as many comments as it can hold"));
    }
    let t = coarse_now();
    let inserted = sqlx::query(
        "INSERT INTO comments (id, node_id, author_id, created_at, enc_body) VALUES (?, ?, ?, ?, ?) \
         ON CONFLICT (id) DO NOTHING",
    )
    .bind(&req.id)
    .bind(&id)
    .bind(&user.id)
    .bind(t)
    .bind(&req.enc_body.0)
    .execute(&state.db)
    .await?;
    if inserted.rows_affected() == 0 {
        return Err(AppError::Conflict("a comment with that id exists".into()));
    }
    Ok((
        StatusCode::CREATED,
        Json(Comment {
            id: req.id,
            node_id: id,
            author_id: user.id,
            author: user.username,
            created_at: t,
            enc_body: req.enc_body,
        }),
    ))
}

pub async fn delete(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<String>,
) -> Result<StatusCode> {
    let row: Option<(String, String)> =
        sqlx::query_as("SELECT node_id, author_id FROM comments WHERE id = ?")
            .bind(&id)
            .fetch_optional(&state.db)
            .await?;
    let Some((node_id, author_id)) = row else {
        return Err(AppError::NotFound);
    };
    // Whoever can't see the node can't learn the comment exists.
    let access = access::access(&state.db, &user.id, &node_id)
        .await?
        .ok_or(AppError::NotFound)?;
    if author_id != user.id && access != Access::Owner {
        return Err(AppError::Forbidden);
    }
    sqlx::query("DELETE FROM comments WHERE id = ?")
        .bind(&id)
        .execute(&state.db)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}
