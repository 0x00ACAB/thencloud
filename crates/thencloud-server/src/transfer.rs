//! Daily transfer limits: how many (encrypted) bytes a user may download
//! and upload per UTC day, if an admin set a limit. File chunks are what
//! counts; listings, keys and the like are too small to matter. A chunk is
//! refused once the day's total has reached the limit, so a day can end up
//! to one chunk over it.

use crate::AppState;
use crate::error::{AppError, Result};
use crate::util::now;
use sqlx::AssertSqlSafe;

#[derive(Clone, Copy)]
pub enum Dir {
    Down,
    Up,
}

impl Dir {
    fn columns(self) -> (&'static str, &'static str) {
        match self {
            Dir::Down => ("daily_download_limit", "down_bytes"),
            Dir::Up => ("daily_upload_limit", "up_bytes"),
        }
    }
}

/// Today, as days since the epoch (UTC).
pub fn today() -> i64 {
    now().div_euclid(86_400)
}

/// When today's counts start again (Unix seconds).
pub fn resets_at() -> i64 {
    (today() + 1) * 86_400
}

/// Refuse if `user_id` has used up today's limit in that direction.
pub async fn check(state: &AppState, user_id: &str, dir: Dir) -> Result<()> {
    let (limit_col, used_col) = dir.columns();
    let row: Option<(Option<i64>, i64)> = sqlx::query_as(AssertSqlSafe(format!(
        "SELECT u.{limit_col}, COALESCE(t.{used_col}, 0) FROM users u \
         LEFT JOIN transfer_usage t ON t.user_id = u.id AND t.day = ? WHERE u.id = ?"
    )))
    .bind(today())
    .bind(user_id)
    .fetch_optional(&state.db)
    .await?;
    match row {
        Some((Some(limit), used)) if used >= limit => Err(AppError::TransferLimit(match dir {
            Dir::Down => "download",
            Dir::Up => "upload",
        })),
        _ => Ok(()),
    }
}

/// Count `bytes` moved by `user_id` today.
pub async fn add(state: &AppState, user_id: &str, dir: Dir, bytes: i64) -> Result<()> {
    let (_, used_col) = dir.columns();
    sqlx::query(AssertSqlSafe(format!(
        "INSERT INTO transfer_usage (user_id, day, {used_col}) VALUES (?, ?, ?) \
         ON CONFLICT (user_id, day) DO UPDATE SET {used_col} = {used_col} + excluded.{used_col}"
    )))
    .bind(user_id)
    .bind(today())
    .bind(bytes)
    .execute(&state.db)
    .await?;
    Ok(())
}

/// Today's (down, up) bytes for a user.
pub async fn used_today(state: &AppState, user_id: &str) -> Result<(i64, i64)> {
    Ok(sqlx::query_as(
        "SELECT down_bytes, up_bytes FROM transfer_usage WHERE user_id = ? AND day = ?",
    )
    .bind(user_id)
    .bind(today())
    .fetch_optional(&state.db)
    .await?
    .unwrap_or((0, 0)))
}

/// Drop counts from before yesterday.
pub async fn prune(state: &AppState) -> Result<u64> {
    Ok(sqlx::query("DELETE FROM transfer_usage WHERE day < ?")
        .bind(today() - 1)
        .execute(&state.db)
        .await?
        .rows_affected())
}
