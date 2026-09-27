//! Administration: users, registration, invites and server stats.
//!
//! Admins manage accounts, never content: nothing here can read a file, a
//! name or a key, and the stats are plain counts.

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use thencloud_crypto::api::*;

use crate::AppState;
use crate::auth::AuthUser;
use crate::error::{AppError, Result};
use crate::routes::{nodes::delete_subtree, uploads::discard};
use crate::settings;
use crate::util::*;

fn require_admin(user: &AuthUser) -> Result<()> {
    if user.is_admin {
        Ok(())
    } else {
        Err(AppError::Forbidden)
    }
}

type UserTuple = (String, String, bool, bool, i64, i64, i64, Option<i64>);

const USER_QUERY: &str = "SELECT u.id, u.username, u.is_admin, u.disabled_at IS NOT NULL, u.quota_bytes, \
     u.used_bytes, u.created_at, (SELECT MAX(s.last_seen) FROM sessions s WHERE s.user_id = u.id) \
     FROM users u";

fn to_admin_user(t: UserTuple) -> AdminUser {
    let (id, username, is_admin, disabled, quota_bytes, used_bytes, created_at, last_seen) = t;
    AdminUser {
        id,
        username,
        is_admin,
        disabled,
        quota_bytes,
        used_bytes,
        created_at,
        last_seen,
    }
}

async fn admin_user(state: &AppState, id: &str) -> Result<AdminUser> {
    let row: Option<UserTuple> = sqlx::query_as(&format!("{USER_QUERY} WHERE u.id = ?"))
        .bind(id)
        .fetch_optional(&state.db)
        .await?;
    row.map(to_admin_user).ok_or(AppError::NotFound)
}

pub async fn users(State(state): State<AppState>, user: AuthUser) -> Result<Json<Vec<AdminUser>>> {
    require_admin(&user)?;
    let rows: Vec<UserTuple> = sqlx::query_as(&format!("{USER_QUERY} ORDER BY u.created_at"))
        .fetch_all(&state.db)
        .await?;
    Ok(Json(rows.into_iter().map(to_admin_user).collect()))
}

pub async fn update_user(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<String>,
    Json(req): Json<UpdateUserRequest>,
) -> Result<Json<AdminUser>> {
    require_admin(&user)?;
    admin_user(&state, &id).await?;
    let own = id == user.id;
    if own && (req.disabled == Some(true) || req.is_admin == Some(false)) {
        return Err(AppError::bad(
            "you can't disable your own account or remove your own admin rights",
        ));
    }
    let mut tx = state.db.begin().await?;
    if let Some(q) = req.quota_bytes {
        if !(0..=1 << 50).contains(&q) {
            return Err(AppError::bad("quota_bytes is out of range"));
        }
        sqlx::query("UPDATE users SET quota_bytes = ? WHERE id = ?")
            .bind(q)
            .bind(&id)
            .execute(&mut *tx)
            .await?;
    }
    if let Some(a) = req.is_admin {
        sqlx::query("UPDATE users SET is_admin = ? WHERE id = ?")
            .bind(a)
            .bind(&id)
            .execute(&mut *tx)
            .await?;
    }
    match req.disabled {
        Some(true) => {
            sqlx::query("UPDATE users SET disabled_at = COALESCE(disabled_at, ?) WHERE id = ?")
                .bind(now())
                .bind(&id)
                .execute(&mut *tx)
                .await?;
            // Signed out everywhere, straight away.
            sqlx::query("DELETE FROM sessions WHERE user_id = ?")
                .bind(&id)
                .execute(&mut *tx)
                .await?;
        }
        Some(false) => {
            sqlx::query("UPDATE users SET disabled_at = NULL WHERE id = ?")
                .bind(&id)
                .execute(&mut *tx)
                .await?;
        }
        None => {}
    }
    tx.commit().await?;
    Ok(Json(admin_user(&state, &id).await?))
}

/// Delete an account and everything in its tree, for good. Files it put in
/// other people's shared folders belong to those people and stay.
pub async fn delete_user(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<String>,
) -> Result<StatusCode> {
    require_admin(&user)?;
    if id == user.id {
        return Err(AppError::bad("you can't delete your own account here"));
    }
    let root: Option<String> = sqlx::query_scalar("SELECT root_node_id FROM users WHERE id = ?")
        .bind(&id)
        .fetch_optional(&state.db)
        .await?;
    let root = root.ok_or(AppError::NotFound)?;
    // Unfinished uploads, including into other people's folders.
    let uploads: Vec<String> = sqlx::query_scalar("SELECT id FROM uploads WHERE user_id = ?")
        .bind(&id)
        .fetch_all(&state.db)
        .await?;
    for u in uploads {
        discard(&state, &u).await?;
    }
    if sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM nodes WHERE id = ?")
        .bind(&root)
        .fetch_one(&state.db)
        .await?
        > 0
    {
        delete_subtree(&state, &root, &id).await?;
    }
    // Sessions, shares, links and invites go via ON DELETE CASCADE.
    sqlx::query("DELETE FROM users WHERE id = ?")
        .bind(&id)
        .execute(&state.db)
        .await?;
    tracing::info!(user = %id, by = %user.username, "user deleted");
    Ok(StatusCode::NO_CONTENT)
}

pub async fn get_settings(
    State(state): State<AppState>,
    user: AuthUser,
) -> Result<Json<AdminSettings>> {
    require_admin(&user)?;
    Ok(Json(AdminSettings {
        registration: settings::registration(&state).await?,
        default_quota: state.config.default_quota,
        downloader: settings::downloader(&state).await?,
        yt_dlp_version: state.downloader.version.clone(),
        downloader_can_merge: state.downloader.can_merge,
        downloader_max_bytes: state.config.downloader_max_bytes,
    }))
}

pub async fn update_settings(
    State(state): State<AppState>,
    user: AuthUser,
    Json(req): Json<UpdateSettingsRequest>,
) -> Result<Json<AdminSettings>> {
    require_admin(&user)?;
    if let Some(r) = req.registration {
        settings::set_registration(&state, r).await?;
    }
    if let Some(d) = req.downloader {
        settings::set_downloader(&state, d).await?;
    }
    get_settings(State(state), user).await
}

type InviteTuple = (String, String, i64, i64, Option<String>, Option<i64>);

pub async fn invites(State(state): State<AppState>, user: AuthUser) -> Result<Json<Vec<Invite>>> {
    require_admin(&user)?;
    let rows: Vec<InviteTuple> = sqlx::query_as(
        "SELECT i.id, COALESCE(c.username, ''), i.created_at, i.expires_at, u.username, i.used_at \
         FROM invites i LEFT JOIN users c ON c.id = i.created_by LEFT JOIN users u ON u.id = i.used_by \
         ORDER BY i.created_at DESC",
    )
    .fetch_all(&state.db)
    .await?;
    Ok(Json(
        rows.into_iter()
            .map(
                |(id, created_by, created_at, expires_at, used_by, used_at)| Invite {
                    id,
                    created_by,
                    created_at,
                    expires_at,
                    used_by,
                    used_at,
                },
            )
            .collect(),
    ))
}

pub async fn create_invite(
    State(state): State<AppState>,
    user: AuthUser,
    Json(req): Json<CreateInviteRequest>,
) -> Result<(StatusCode, Json<CreatedInvite>)> {
    require_admin(&user)?;
    if !(1..=90).contains(&req.days) {
        return Err(AppError::bad("days must be between 1 and 90"));
    }
    let token = random_token(24);
    let id = new_uuid();
    let t = now();
    let expires_at = t + req.days * 86400;
    sqlx::query(
        "INSERT INTO invites (id, token_hash, created_by, created_at, expires_at) VALUES (?, ?, ?, ?, ?)",
    )
    .bind(&id)
    .bind(sha256(token.as_bytes()))
    .bind(&user.id)
    .bind(t)
    .bind(expires_at)
    .execute(&state.db)
    .await?;
    let invite = Invite {
        id,
        created_by: user.username,
        created_at: t,
        expires_at,
        used_by: None,
        used_at: None,
    };
    Ok((StatusCode::CREATED, Json(CreatedInvite { invite, token })))
}

pub async fn delete_invite(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<String>,
) -> Result<StatusCode> {
    require_admin(&user)?;
    let r = sqlx::query("DELETE FROM invites WHERE id = ?")
        .bind(&id)
        .execute(&state.db)
        .await?;
    if r.rows_affected() == 0 {
        return Err(AppError::NotFound);
    }
    Ok(StatusCode::NO_CONTENT)
}

pub async fn stats(State(state): State<AppState>, user: AuthUser) -> Result<Json<ServerStats>> {
    require_admin(&user)?;
    let count = |sql: &'static str| {
        let db = state.db.clone();
        async move { sqlx::query_scalar::<_, i64>(sql).fetch_one(&db).await }
    };
    Ok(Json(ServerStats {
        users: count("SELECT COUNT(*) FROM users").await?,
        disabled_users: count("SELECT COUNT(*) FROM users WHERE disabled_at IS NOT NULL").await?,
        active_sessions: sqlx::query_scalar("SELECT COUNT(*) FROM sessions WHERE expires_at > ?")
            .bind(now())
            .fetch_one(&state.db)
            .await?,
        used_bytes: count("SELECT COALESCE(SUM(used_bytes), 0) FROM users").await?,
        quota_bytes: count("SELECT COALESCE(SUM(quota_bytes), 0) FROM users").await?,
        files: count("SELECT COUNT(*) FROM nodes WHERE kind = 'file'").await?,
        folders: count(
            "SELECT COUNT(*) FROM nodes WHERE kind = 'folder' AND parent_id IS NOT NULL",
        )
        .await?,
        versions: count("SELECT COUNT(*) FROM file_versions").await?,
        shares: count("SELECT COUNT(*) FROM shares").await?,
        public_links: count("SELECT COUNT(*) FROM public_links").await?,
    }))
}
