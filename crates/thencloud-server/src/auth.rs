//! Request extractors: bearer-token sessions and client IP.

use std::convert::Infallible;
use std::net::{IpAddr, SocketAddr};

use axum::extract::{ConnectInfo, FromRequestParts};
use axum::http::Method;
use axum::http::header::AUTHORIZATION;
use axum::http::request::Parts;

use crate::AppState;
use crate::error::{AppError, Result};
use crate::util::{now, random_token, sha256};

/// An authenticated user, resolved from `Authorization: Bearer <token>`.
#[derive(Debug, Clone)]
pub struct AuthUser {
    pub id: String,
    pub username: String,
    pub is_admin: bool,
    pub token_hash: Vec<u8>,
    /// Set for sessions signed in with an app password.
    pub app_password_id: Option<String>,
}

impl FromRequestParts<AppState> for AuthUser {
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self> {
        let token = parts
            .headers
            .get(AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.strip_prefix("Bearer "))
            .map(str::trim)
            .filter(|t| !t.is_empty())
            .ok_or(AppError::Unauthorized)?;
        let token_hash = sha256(token.as_bytes());
        let t = now();
        #[derive(sqlx::FromRow)]
        struct Row {
            id: String,
            username: String,
            is_admin: bool,
            last_seen: i64,
            app_password_id: Option<String>,
            scope: Option<String>,
        }
        let row: Option<Row> = sqlx::query_as(
            "SELECT u.id, u.username, u.is_admin, s.last_seen, s.app_password_id, a.scope \
             FROM sessions s JOIN users u ON u.id = s.user_id \
             LEFT JOIN app_passwords a ON a.id = s.app_password_id \
             WHERE s.token_hash = ? AND s.expires_at > ? AND u.disabled_at IS NULL",
        )
        .bind(&token_hash)
        .bind(t)
        .fetch_optional(&state.db)
        .await?;
        let Row {
            id,
            username,
            is_admin,
            last_seen,
            app_password_id,
            scope,
        } = row.ok_or(AppError::Unauthorized)?;
        // A read-only app password can look, download and sign itself out.
        if scope.as_deref() == Some("read")
            && !matches!(parts.method, Method::GET | Method::HEAD)
            && !parts.uri.path().ends_with("/auth/logout")
        {
            return Err(AppError::Forbidden);
        }
        if t - last_seen > 60 {
            sqlx::query("UPDATE sessions SET last_seen = ?, expires_at = ? WHERE token_hash = ?")
                .bind(t)
                .bind(t + state.config.session_days * 86400)
                .bind(&token_hash)
                .execute(&state.db)
                .await?;
        }
        Ok(AuthUser {
            id,
            username,
            is_admin,
            token_hash,
            app_password_id,
        })
    }
}

/// Peer address, if the server was started with connect info.
pub struct ClientIp(pub Option<IpAddr>);

impl<S: Send + Sync> FromRequestParts<S> for ClientIp {
    type Rejection = Infallible;

    async fn from_request_parts(parts: &mut Parts, _: &S) -> std::result::Result<Self, Infallible> {
        Ok(ClientIp(
            parts
                .extensions
                .get::<ConnectInfo<SocketAddr>>()
                .map(|c| c.0.ip()),
        ))
    }
}

impl ClientIp {
    pub fn key(&self) -> String {
        self.0
            .map(|ip| ip.to_string())
            .unwrap_or_else(|| "unknown".into())
    }
}

/// Create a session and return its bearer token. Only the token's hash is
/// stored.
pub async fn create_session(
    state: &AppState,
    user_id: &str,
    device: Option<&str>,
    app_password_id: Option<&str>,
) -> Result<String> {
    let token = random_token(32);
    let t = now();
    let device: String = device.unwrap_or("unknown").chars().take(100).collect();
    sqlx::query(
        "INSERT INTO sessions (token_hash, id, user_id, device_name, created_at, last_seen, expires_at, \
         app_password_id) VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(sha256(token.as_bytes()))
    .bind(random_token(16))
    .bind(user_id)
    .bind(device)
    .bind(t)
    .bind(t)
    .bind(t + state.config.session_days * 86400)
    .bind(app_password_id)
    .execute(&state.db)
    .await?;
    Ok(token)
}
