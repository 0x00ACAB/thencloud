//! Request extractors: bearer-token sessions and client IP.

use std::convert::Infallible;
use std::net::{IpAddr, SocketAddr};

use axum::extract::{ConnectInfo, FromRequestParts};
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
        let row: Option<(String, String, bool, i64)> = sqlx::query_as(
            "SELECT u.id, u.username, u.is_admin, s.last_seen FROM sessions s \
             JOIN users u ON u.id = s.user_id WHERE s.token_hash = ? AND s.expires_at > ?",
        )
        .bind(&token_hash)
        .bind(t)
        .fetch_optional(&state.db)
        .await?;
        let (id, username, is_admin, last_seen) = row.ok_or(AppError::Unauthorized)?;
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
) -> Result<String> {
    let token = random_token(32);
    let t = now();
    let device: String = device.unwrap_or("unknown").chars().take(100).collect();
    sqlx::query(
        "INSERT INTO sessions (token_hash, id, user_id, device_name, created_at, last_seen, expires_at) \
         VALUES (?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(sha256(token.as_bytes()))
    .bind(random_token(16))
    .bind(user_id)
    .bind(device)
    .bind(t)
    .bind(t)
    .bind(t + state.config.session_days * 86400)
    .execute(&state.db)
    .await?;
    Ok(token)
}
