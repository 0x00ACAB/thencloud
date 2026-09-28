//! Two-factor sign-in: authenticator app codes (TOTP) and passkeys, as a
//! gate on password sign-in. Neither holds a key: the server checks them and
//! only then hands out the wrapped master key.
//!
//! An account with a TOTP secret or any passkey gets a ticket instead of a
//! session when the password is right, and trades it for a session with a
//! code or a passkey assertion. App passwords and the recovery key don't
//! ask for a second factor: they are high-entropy secrets of their own, and
//! the recovery key is the way back in when everything else is lost.

use axum::Json;
use axum::extract::State;
use thencloud_crypto::api::*;

use crate::AppState;
use crate::auth::{AuthUser, ClientIp, create_session};
use crate::error::{AppError, Result};
use crate::routes::auth::{UserRow, user_by_id, verify_current};
use crate::routes::passkeys;
use crate::totp;
use crate::util::*;

/// How long a half-finished sign-in or a setup waits.
const TICKET_SECS: i64 = 5 * 60;

#[derive(sqlx::FromRow)]
pub(crate) struct Challenge {
    pub user_id: Option<String>,
    pub challenge: Vec<u8>,
    pub data: Option<Vec<u8>>,
}

fn challenge_id(token: &str) -> String {
    thencloud_crypto::b64_encode(&sha256(token.as_bytes()))
}

/// Store short-lived state and return the token that names it. Only a hash
/// of the token is kept.
pub(crate) async fn put_challenge(
    state: &AppState,
    user_id: Option<&str>,
    purpose: &str,
    challenge: &[u8],
    data: Option<&[u8]>,
) -> Result<String> {
    let token = random_token(32);
    sqlx::query(
        "INSERT INTO auth_challenges (id, user_id, purpose, challenge, data, expires_at) \
         VALUES (?, ?, ?, ?, ?, ?)",
    )
    .bind(challenge_id(&token))
    .bind(user_id)
    .bind(purpose)
    .bind(challenge)
    .bind(data)
    .bind(now() + TICKET_SECS)
    .execute(&state.db)
    .await?;
    Ok(token)
}

pub(crate) async fn get_challenge(
    state: &AppState,
    token: &str,
    purpose: &str,
) -> Result<Challenge> {
    sqlx::query_as(
        "SELECT user_id, challenge, data FROM auth_challenges \
         WHERE id = ? AND purpose = ? AND expires_at > ?",
    )
    .bind(challenge_id(token))
    .bind(purpose)
    .bind(now())
    .fetch_optional(&state.db)
    .await?
    .ok_or(AppError::SignInExpired)
}

pub(crate) async fn drop_challenge(state: &AppState, token: &str) -> Result<()> {
    sqlx::query("DELETE FROM auth_challenges WHERE id = ?")
        .bind(challenge_id(token))
        .execute(&state.db)
        .await?;
    Ok(())
}

/// Called after a correct password: a ticket if the account needs a second
/// factor, else None.
pub(crate) async fn login_challenge(
    state: &AppState,
    user: &UserRow,
    device: Option<&str>,
) -> Result<Option<SecondFactorChallenge>> {
    let creds = passkeys::credential_ids(state, &user.id).await?;
    let totp = user.totp_secret.is_some();
    if !totp && creds.is_empty() {
        return Ok(None);
    }
    let challenge = thencloud_crypto::random_bytes(32);
    let device = device.map(|d| d.as_bytes());
    let ticket = put_challenge(state, Some(&user.id), "login", &challenge, device).await?;
    Ok(Some(SecondFactorChallenge {
        ticket,
        totp,
        passkey: (!creds.is_empty()).then(|| PasskeyRequest {
            challenge: B64(challenge),
            allow_credentials: creds.into_iter().map(B64).collect(),
        }),
    }))
}

/// Finish a password sign-in with a TOTP code or a passkey.
pub async fn verify(
    State(state): State<AppState>,
    ip: ClientIp,
    Json(req): Json<SecondFactorRequest>,
) -> Result<Json<SessionResponse>> {
    let ikey = ip.key().map(|k| format!("2fa-ip:{k}"));
    if ikey.as_deref().is_some_and(|k| state.limiter.blocked(k)) {
        return Err(AppError::RateLimited);
    }
    let ch = get_challenge(&state, &req.ticket, "login").await?;
    let user_id = ch.user_id.ok_or(AppError::SignInExpired)?;
    let ukey = format!("2fa-user:{user_id}");
    if state.limiter.blocked(&ukey) {
        return Err(AppError::RateLimited);
    }
    let user = user_by_id(&state, &user_id).await?;
    let ok = match (&req.totp_code, &req.passkey) {
        (Some(code), None) => check_totp(&state, &user, code).await?,
        (None, Some(a)) => passkeys::check_assertion(&state, &user.id, a, &ch.challenge, false)
            .await
            .is_ok(),
        _ => return Err(AppError::bad("send either totp_code or passkey")),
    };
    if !ok {
        state.limiter.fail(&ukey);
        if let Some(k) = &ikey {
            state.limiter.fail(k);
        }
        return Err(AppError::InvalidSecondFactor);
    }
    state.limiter.clear(&ukey);
    drop_challenge(&state, &req.ticket).await?;
    if user.disabled_at.is_some() {
        return Err(AppError::AccountDisabled);
    }
    let device = ch.data.map(|d| String::from_utf8_lossy(&d).into_owned());
    let token = create_session(&state, &user.id, device.as_deref(), None).await?;
    Ok(Json(SessionResponse {
        token,
        me: user.into_me(&state.config)?,
    }))
}

/// Check a code and use it up, so it can't be replayed.
async fn check_totp(state: &AppState, user: &UserRow, code: &str) -> Result<bool> {
    let Some(secret) = &user.totp_secret else {
        return Ok(false);
    };
    let Some(step) = totp::check(secret, code, now(), user.totp_last_step) else {
        return Ok(false);
    };
    let used = sqlx::query(
        "UPDATE users SET totp_last_step = ? WHERE id = ? AND (totp_last_step IS NULL OR totp_last_step < ?)",
    )
    .bind(step)
    .bind(&user.id)
    .bind(step)
    .execute(&state.db)
    .await?;
    Ok(used.rows_affected() == 1)
}

/// Start setting up an authenticator app: a new secret, kept aside until a
/// code from the app confirms it.
pub async fn totp_setup(
    State(state): State<AppState>,
    user: AuthUser,
    Json(req): Json<CurrentPassword>,
) -> Result<Json<TotpSetup>> {
    if user.app_password_id.is_some() {
        return Err(AppError::Forbidden);
    }
    verify_current(&state, &user, &req.current_auth_key).await?;
    let secret = thencloud_crypto::random_bytes(totp::SECRET_LEN);
    let setup_id = put_challenge(&state, Some(&user.id), "totp-setup", &secret, None).await?;
    Ok(Json(TotpSetup {
        setup_id,
        secret: totp::base32(&secret),
    }))
}

pub async fn totp_enable(
    State(state): State<AppState>,
    user: AuthUser,
    Json(req): Json<EnableTotpRequest>,
) -> Result<Json<Me>> {
    let key = format!("totp-setup:{}", user.id);
    if state.limiter.blocked(&key) {
        return Err(AppError::RateLimited);
    }
    let ch = get_challenge(&state, &req.setup_id, "totp-setup").await?;
    if ch.user_id.as_deref() != Some(&user.id) {
        return Err(AppError::SignInExpired);
    }
    let t = now();
    let Some(step) = totp::check(&ch.challenge, &req.code, t, None) else {
        state.limiter.fail(&key);
        return Err(AppError::InvalidSecondFactor);
    };
    drop_challenge(&state, &req.setup_id).await?;
    sqlx::query(
        "UPDATE users SET totp_secret = ?, totp_created_at = ?, totp_last_step = ? WHERE id = ?",
    )
    .bind(&ch.challenge)
    .bind(t)
    .bind(step)
    .bind(&user.id)
    .execute(&state.db)
    .await?;
    Ok(Json(
        user_by_id(&state, &user.id).await?.into_me(&state.config)?,
    ))
}

pub async fn totp_disable(
    State(state): State<AppState>,
    user: AuthUser,
    Json(req): Json<CurrentPassword>,
) -> Result<Json<Me>> {
    if user.app_password_id.is_some() {
        return Err(AppError::Forbidden);
    }
    verify_current(&state, &user, &req.current_auth_key).await?;
    sqlx::query(
        "UPDATE users SET totp_secret = NULL, totp_created_at = NULL, totp_last_step = NULL WHERE id = ?",
    )
    .bind(&user.id)
    .execute(&state.db)
    .await?;
    Ok(Json(
        user_by_id(&state, &user.id).await?.into_me(&state.config)?,
    ))
}
