//! A health check for load balancers and container runtimes, and metrics in
//! the Prometheus text format with the same counts the admin view shows,
//! nothing more.

use axum::extract::State;
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};

use crate::AppState;
use crate::error::{AppError, Result};
use crate::routes::admin::server_stats;
use crate::util::*;

/// `GET /api/health`: 200 when the database answers and the data directory
/// is there, 503 otherwise. Needs no sign-in and says nothing else.
pub async fn health(State(state): State<AppState>) -> Response {
    let db = sqlx::query_scalar::<_, i64>("SELECT 1")
        .fetch_one(&state.db)
        .await
        .is_ok();
    let disk = tokio::fs::metadata(&state.config.data_dir)
        .await
        .is_ok_and(|m| m.is_dir());
    if db && disk {
        (StatusCode::OK, "ok\n").into_response()
    } else {
        (StatusCode::SERVICE_UNAVAILABLE, "unavailable\n").into_response()
    }
}

/// `GET /api/metrics`: off unless `--metrics-token` is set, and then only
/// with `Authorization: Bearer <that token>`.
pub async fn metrics(State(state): State<AppState>, headers: HeaderMap) -> Result<Response> {
    let Some(expected) = state
        .config
        .metrics_token
        .as_deref()
        .filter(|t| !t.is_empty())
    else {
        return Err(AppError::NotFound);
    };
    let given = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .unwrap_or("");
    // Compared through an HMAC so the time taken says nothing about the token.
    if !hmac_verify(
        &state.secret[..],
        given.as_bytes(),
        &hmac(&state.secret[..], expected.as_bytes()),
    ) {
        return Err(AppError::Unauthorized);
    }
    let s = server_stats(&state).await?;
    let mut out = String::new();
    let mut gauge = |name: &str, help: &str, value: i64| {
        out.push_str(&format!(
            "# HELP thencloud_{name} {help}\n# TYPE thencloud_{name} gauge\nthencloud_{name} {value}\n"
        ));
    };
    gauge("users", "Accounts, including disabled ones.", s.users);
    gauge("disabled_users", "Disabled accounts.", s.disabled_users);
    gauge(
        "active_sessions",
        "Sessions that haven't expired.",
        s.active_sessions,
    );
    gauge(
        "used_bytes",
        "Stored ciphertext, in bytes, across all accounts.",
        s.used_bytes,
    );
    gauge(
        "quota_bytes",
        "Sum of all accounts' quotas, in bytes.",
        s.quota_bytes,
    );
    gauge("files", "Files, including those in the trash.", s.files);
    gauge(
        "folders",
        "Folders, not counting each account's root.",
        s.folders,
    );
    gauge("versions", "Stored file versions.", s.versions);
    gauge("shares", "Shares between users.", s.shares);
    gauge("public_links", "Public links.", s.public_links);
    Ok((
        [(
            header::CONTENT_TYPE,
            "text/plain; version=0.0.4; charset=utf-8",
        )],
        out,
    )
        .into_response())
}
