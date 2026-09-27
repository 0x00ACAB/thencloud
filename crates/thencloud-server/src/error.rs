use axum::Json;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use thencloud_crypto::api::ErrorBody;

#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("{0}")]
    BadRequest(String),
    #[error("authentication required")]
    Unauthorized,
    #[error("invalid username or password")]
    InvalidCredentials,
    #[error("this link is password protected")]
    PasswordRequired,
    #[error("you do not have permission to do that")]
    Forbidden,
    #[error("not found")]
    NotFound,
    #[error("{0}")]
    Conflict(String),
    #[error("storage quota exceeded")]
    QuotaExceeded,
    #[error("too many failed attempts, try again later")]
    RateLimited,
    #[error("registration is disabled on this server")]
    RegistrationClosed,
    #[error("database error")]
    Db(#[from] sqlx::Error),
    #[error("storage error")]
    Io(#[from] std::io::Error),
    #[error("internal error: {0}")]
    Internal(String),
}

pub type Result<T> = std::result::Result<T, AppError>;

impl AppError {
    pub fn bad(msg: impl Into<String>) -> Self {
        AppError::BadRequest(msg.into())
    }

    fn parts(&self) -> (StatusCode, &'static str) {
        use AppError::*;
        match self {
            BadRequest(_) => (StatusCode::BAD_REQUEST, "bad_request"),
            Unauthorized => (StatusCode::UNAUTHORIZED, "unauthorized"),
            InvalidCredentials => (StatusCode::UNAUTHORIZED, "invalid_credentials"),
            PasswordRequired => (StatusCode::UNAUTHORIZED, "password_required"),
            Forbidden => (StatusCode::FORBIDDEN, "forbidden"),
            NotFound => (StatusCode::NOT_FOUND, "not_found"),
            Conflict(_) => (StatusCode::CONFLICT, "conflict"),
            QuotaExceeded => (StatusCode::INSUFFICIENT_STORAGE, "quota_exceeded"),
            RateLimited => (StatusCode::TOO_MANY_REQUESTS, "rate_limited"),
            RegistrationClosed => (StatusCode::FORBIDDEN, "registration_closed"),
            Db(_) | Io(_) | Internal(_) => (StatusCode::INTERNAL_SERVER_ERROR, "internal"),
        }
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let (status, code) = self.parts();
        if status.is_server_error() {
            match &self {
                AppError::Db(e) => tracing::error!(error = %e, "database error"),
                AppError::Io(e) => tracing::error!(error = %e, "io error"),
                e => tracing::error!(error = %e, "internal error"),
            }
        }
        let body = ErrorBody {
            error: code.to_string(),
            message: self.to_string(),
        };
        (status, Json(body)).into_response()
    }
}

/// True if a database error is a UNIQUE / PRIMARY KEY constraint violation.
pub fn is_unique_violation(e: &sqlx::Error) -> bool {
    matches!(e, sqlx::Error::Database(d) if d.is_unique_violation())
}

pub fn is_fk_violation(e: &sqlx::Error) -> bool {
    matches!(e, sqlx::Error::Database(d) if d.is_foreign_key_violation())
}
