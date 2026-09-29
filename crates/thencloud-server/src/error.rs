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
    #[error("that code or passkey didn't work")]
    InvalidSecondFactor,
    #[error("that took too long; start again")]
    SignInExpired,
    #[error("this link is password protected")]
    PasswordRequired,
    #[error("you do not have permission to do that")]
    Forbidden,
    #[error("not found")]
    NotFound,
    #[error("{0}")]
    Conflict(String),
    #[error("the original folder is in the trash or was deleted")]
    ParentUnavailable,
    #[error("there's already something with that name here")]
    NameTaken,
    #[error("storage quota exceeded")]
    QuotaExceeded,
    #[error("too many failed attempts, try again later")]
    RateLimited,
    /// "download" or "upload".
    #[error("you've reached today's {0} limit on this server; it starts again at midnight UTC")]
    TransferLimit(&'static str),
    #[error("registration is disabled on this server")]
    RegistrationClosed,
    #[error("the check that you're a person didn't go through; try again")]
    TurnstileFailed,
    #[error("this account has been disabled by an administrator")]
    AccountDisabled,
    #[error("this invite link is invalid, used or expired")]
    InvalidInvite,
    #[error("{0}")]
    Busy(String),
    #[error("{0}")]
    Unavailable(String),
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
            InvalidSecondFactor => (StatusCode::UNAUTHORIZED, "invalid_second_factor"),
            SignInExpired => (StatusCode::UNAUTHORIZED, "sign_in_expired"),
            PasswordRequired => (StatusCode::UNAUTHORIZED, "password_required"),
            Forbidden => (StatusCode::FORBIDDEN, "forbidden"),
            NotFound => (StatusCode::NOT_FOUND, "not_found"),
            Conflict(_) => (StatusCode::CONFLICT, "conflict"),
            ParentUnavailable => (StatusCode::CONFLICT, "parent_unavailable"),
            NameTaken => (StatusCode::CONFLICT, "name_taken"),
            QuotaExceeded => (StatusCode::INSUFFICIENT_STORAGE, "quota_exceeded"),
            RateLimited => (StatusCode::TOO_MANY_REQUESTS, "rate_limited"),
            TransferLimit(_) => (StatusCode::TOO_MANY_REQUESTS, "transfer_limit"),
            RegistrationClosed => (StatusCode::FORBIDDEN, "registration_closed"),
            TurnstileFailed => (StatusCode::FORBIDDEN, "turnstile_failed"),
            AccountDisabled => (StatusCode::FORBIDDEN, "account_disabled"),
            InvalidInvite => (StatusCode::FORBIDDEN, "invalid_invite"),
            Busy(_) => (StatusCode::TOO_MANY_REQUESTS, "busy"),
            Unavailable(_) => (StatusCode::SERVICE_UNAVAILABLE, "unavailable"),
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

/// A duplicate name tag becomes `NameTaken`; anything else stays a database error.
pub fn name_conflict(e: sqlx::Error) -> AppError {
    match &e {
        sqlx::Error::Database(d) if d.is_unique_violation() && d.message().contains("name_tag") => {
            AppError::NameTaken
        }
        _ => AppError::Db(e),
    }
}

/// True if a database error is a UNIQUE / PRIMARY KEY constraint violation.
pub fn is_unique_violation(e: &sqlx::Error) -> bool {
    matches!(e, sqlx::Error::Database(d) if d.is_unique_violation())
}

pub fn is_fk_violation(e: &sqlx::Error) -> bool {
    matches!(e, sqlx::Error::Database(d) if d.is_foreign_key_violation())
}
