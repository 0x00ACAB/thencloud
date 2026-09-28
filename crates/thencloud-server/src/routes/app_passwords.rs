//! App passwords: credentials for sync clients and other devices that are
//! never the account password. Each wraps its own copy of the master key,
//! can be read-only, and can be revoked on its own, which signs out every
//! session it started.

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use thencloud_crypto::KEY_LEN;
use thencloud_crypto::api::*;

use crate::AppState;
use crate::auth::{AuthUser, ClientIp, create_session};
use crate::error::{AppError, Result, is_unique_violation};
use crate::routes::auth::{user_by_id, verify_current};
use crate::util::*;

const MAX_PER_USER: i64 = 50;

fn scope_str(s: AppScope) -> &'static str {
    match s {
        AppScope::Full => "full",
        AppScope::Read => "read",
    }
}

fn parse_scope(s: &str) -> AppScope {
    if s == "read" {
        AppScope::Read
    } else {
        AppScope::Full
    }
}

pub async fn list(State(state): State<AppState>, user: AuthUser) -> Result<Json<Vec<AppPassword>>> {
    let rows: Vec<(String, String, String, i64, Option<i64>)> = sqlx::query_as(
        "SELECT id, name, scope, created_at, last_used_at FROM app_passwords \
         WHERE user_id = ? ORDER BY created_at",
    )
    .bind(&user.id)
    .fetch_all(&state.db)
    .await?;
    Ok(Json(
        rows.into_iter()
            .map(|(id, name, scope, created_at, last_used_at)| AppPassword {
                id,
                name,
                scope: parse_scope(&scope),
                created_at,
                last_used_at,
            })
            .collect(),
    ))
}

/// Needs the account password, so a stolen app password can't mint more.
pub async fn create(
    State(state): State<AppState>,
    user: AuthUser,
    Json(req): Json<CreateAppPasswordRequest>,
) -> Result<(StatusCode, Json<AppPassword>)> {
    check_id(&req.id, "id")?;
    check_len(&req.auth_key, KEY_LEN, "auth_key")?;
    check_len(&req.enc_master_key, WRAPPED_KEY_LEN, "enc_master_key")?;
    let name = req.name.trim();
    if name.is_empty() || name.chars().count() > 100 {
        return Err(AppError::bad("name must be 1 to 100 characters"));
    }
    if user.app_password_id.is_some() {
        return Err(AppError::Forbidden);
    }
    verify_current(&state, &user, &req.current_auth_key).await?;
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM app_passwords WHERE user_id = ?")
        .bind(&user.id)
        .fetch_one(&state.db)
        .await?;
    if count >= MAX_PER_USER {
        return Err(AppError::bad(format!(
            "you can have at most {MAX_PER_USER} app passwords"
        )));
    }
    let t = now();
    let res = sqlx::query(
        "INSERT INTO app_passwords (id, user_id, name, scope, auth_hash, enc_master_key, created_at) \
         VALUES (?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&req.id)
    .bind(&user.id)
    .bind(name)
    .bind(scope_str(req.scope))
    .bind(sha256(&req.auth_key.0))
    .bind(&req.enc_master_key.0)
    .bind(t)
    .execute(&state.db)
    .await;
    match res {
        Err(e) if is_unique_violation(&e) => {
            return Err(AppError::Conflict("app password already exists".into()));
        }
        r => r?,
    };
    Ok((
        StatusCode::CREATED,
        Json(AppPassword {
            id: req.id,
            name: name.into(),
            scope: req.scope,
            created_at: t,
            last_used_at: None,
        }),
    ))
}

/// Revoke an app password and sign out its sessions.
pub async fn delete(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<String>,
) -> Result<StatusCode> {
    // A device signed in with an app password may only revoke that one.
    if user.app_password_id.as_ref().is_some_and(|own| *own != id) {
        return Err(AppError::Forbidden);
    }
    let r = sqlx::query("DELETE FROM app_passwords WHERE id = ? AND user_id = ?")
        .bind(&id)
        .bind(&user.id)
        .execute(&state.db)
        .await?;
    if r.rows_affected() == 0 {
        return Err(AppError::NotFound);
    }
    Ok(StatusCode::NO_CONTENT)
}

/// Sign in with an app password alone: its auth half identifies it.
pub async fn login(
    State(state): State<AppState>,
    ip: ClientIp,
    Json(req): Json<AppLoginRequest>,
) -> Result<Json<AppLoginResponse>> {
    let ikey = ip.key().map(|k| format!("app-login-ip:{k}"));
    if ikey.as_deref().is_some_and(|k| state.limiter.blocked(k)) {
        return Err(AppError::RateLimited);
    }
    let row: Option<(String, String, String, Vec<u8>)> = sqlx::query_as(
        "SELECT id, user_id, scope, enc_master_key FROM app_passwords WHERE auth_hash = ?",
    )
    .bind(sha256(&req.auth_key.0))
    .fetch_optional(&state.db)
    .await?;
    let Some((id, user_id, scope, enc_master_key)) = row else {
        if let Some(k) = &ikey {
            state.limiter.fail(k);
        }
        return Err(AppError::InvalidCredentials);
    };
    let u = user_by_id(&state, &user_id).await?;
    if u.disabled_at.is_some() {
        return Err(AppError::AccountDisabled);
    }
    sqlx::query("UPDATE app_passwords SET last_used_at = ? WHERE id = ?")
        .bind(now())
        .bind(&id)
        .execute(&state.db)
        .await?;
    let token = create_session(&state, &user_id, req.device_name.as_deref(), Some(&id)).await?;
    Ok(Json(AppLoginResponse {
        token,
        me: u.into_me(&state.config)?,
        app_password_id: id,
        scope: parse_scope(&scope),
        enc_master_key: B64(enc_master_key),
    }))
}
