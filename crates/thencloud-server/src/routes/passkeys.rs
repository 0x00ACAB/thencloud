//! Passkeys: a second factor after the password and, when the authenticator
//! supports the PRF extension, a way to sign in without it. For that the
//! browser derives a key from the passkey's PRF output and wraps the master
//! key under it; the server stores the wrapped key and gives it out only
//! after a valid assertion, and can't unwrap it itself.

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use thencloud_crypto::api::*;

use crate::AppState;
use crate::auth::{AuthUser, ClientIp, create_session};
use crate::error::{AppError, Result, is_unique_violation};
use crate::routes::auth::{user_by_id, verify_current};
use crate::routes::two_factor::{drop_challenge, get_challenge, put_challenge};
use crate::util::*;
use crate::webauthn;

const MAX_PER_USER: i64 = 20;
/// How long a passkey sign-in challenge is good for.
const CHALLENGE_SECS: i64 = 5 * 60;

pub(crate) async fn credential_ids(state: &AppState, user_id: &str) -> Result<Vec<Vec<u8>>> {
    Ok(sqlx::query_scalar(
        "SELECT credential_id FROM passkeys WHERE user_id = ? ORDER BY created_at",
    )
    .bind(user_id)
    .fetch_all(&state.db)
    .await?)
}

#[derive(sqlx::FromRow)]
struct Row {
    id: String,
    user_id: String,
    public_key: Vec<u8>,
    rp_id: String,
    sign_count: i64,
    enc_master_key: Option<Vec<u8>>,
}

/// Verify an assertion from one of `user_id`'s passkeys (or anyone's, for
/// a sign-in with the passkey alone) and record its use.
pub(crate) async fn check_assertion(
    state: &AppState,
    user_id: &str,
    a: &PasskeyAssertion,
    challenge: &[u8],
    user_verified: bool,
) -> Result<(String, Option<Vec<u8>>)> {
    let row: Option<Row> = sqlx::query_as(
        "SELECT id, user_id, public_key, rp_id, sign_count, enc_master_key FROM passkeys \
         WHERE credential_id = ?",
    )
    .bind(&a.credential_id.0)
    .fetch_optional(&state.db)
    .await?;
    let row = row
        .filter(|r| user_id.is_empty() || r.user_id == user_id)
        .ok_or(AppError::InvalidCredentials)?;
    if a.user_handle
        .as_ref()
        .is_some_and(|h| !h.is_empty() && h.0 != row.user_id.as_bytes())
    {
        return Err(AppError::InvalidCredentials);
    }
    let count = webauthn::assert(
        &webauthn::Stored {
            public_key: &row.public_key,
            rp_id: &row.rp_id,
            sign_count: row.sign_count as u32,
        },
        &a.client_data_json,
        &a.authenticator_data,
        &a.signature,
        challenge,
        user_verified,
    )?;
    sqlx::query("UPDATE passkeys SET sign_count = ?, last_used_at = ? WHERE id = ?")
        .bind(count as i64)
        .bind(now())
        .bind(&row.id)
        .execute(&state.db)
        .await?;
    Ok((row.user_id, row.enc_master_key))
}

#[derive(sqlx::FromRow)]
struct ListRow {
    id: String,
    name: String,
    credential_id: Vec<u8>,
    unlock: bool,
    created_at: i64,
    last_used_at: Option<i64>,
}

pub async fn list(State(state): State<AppState>, user: AuthUser) -> Result<Json<Vec<Passkey>>> {
    let rows: Vec<ListRow> = sqlx::query_as(
        "SELECT id, name, credential_id, enc_master_key IS NOT NULL AS unlock, created_at, \
         last_used_at FROM passkeys WHERE user_id = ? ORDER BY created_at",
    )
    .bind(&user.id)
    .fetch_all(&state.db)
    .await?;
    Ok(Json(
        rows.into_iter()
            .map(|r| Passkey {
                id: r.id,
                name: r.name,
                credential_id: B64(r.credential_id),
                unlock: r.unlock,
                created_at: r.created_at,
                last_used_at: r.last_used_at,
            })
            .collect(),
    ))
}

/// Step one of adding a passkey: a challenge and the credentials the
/// authenticator should refuse to make again.
pub async fn creation_options(
    State(state): State<AppState>,
    user: AuthUser,
) -> Result<Json<PasskeyCreationOptions>> {
    if user.app_password_id.is_some() {
        return Err(AppError::Forbidden);
    }
    let challenge = thencloud_crypto::random_bytes(32);
    let registration_id =
        put_challenge(&state, Some(&user.id), "passkey-register", &challenge, None).await?;
    Ok(Json(PasskeyCreationOptions {
        registration_id,
        challenge: B64(challenge),
        user_handle: B64(user.id.as_bytes().to_vec()),
        exclude_credentials: credential_ids(&state, &user.id)
            .await?
            .into_iter()
            .map(B64)
            .collect(),
    }))
}

/// Step two: the authenticator's response. Needs the account password, like
/// every other way into the account.
pub async fn register(
    State(state): State<AppState>,
    user: AuthUser,
    Json(req): Json<RegisterPasskeyRequest>,
) -> Result<(StatusCode, Json<Passkey>)> {
    if user.app_password_id.is_some() {
        return Err(AppError::Forbidden);
    }
    let name = req.name.trim();
    if name.is_empty() || name.chars().count() > 100 {
        return Err(AppError::bad("name must be 1 to 100 characters"));
    }
    if let Some(k) = &req.enc_master_key {
        check_len(k, WRAPPED_KEY_LEN, "enc_master_key")?;
    }
    verify_current(&state, &user, &req.current_auth_key).await?;
    let ch = get_challenge(&state, &req.registration_id, "passkey-register").await?;
    drop_challenge(&state, &req.registration_id).await?;
    if ch.user_id.as_deref() != Some(&user.id) {
        return Err(AppError::SignInExpired);
    }
    let cred = webauthn::register(
        &req.client_data_json,
        &req.attestation_object,
        &ch.challenge,
    )?;
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM passkeys WHERE user_id = ?")
        .bind(&user.id)
        .fetch_one(&state.db)
        .await?;
    if count >= MAX_PER_USER {
        return Err(AppError::bad(format!(
            "you can have at most {MAX_PER_USER} passkeys"
        )));
    }
    let id = new_uuid();
    let t = now();
    let res = sqlx::query(
        "INSERT INTO passkeys (id, user_id, credential_id, public_key, rp_id, sign_count, name, \
         enc_master_key, created_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&id)
    .bind(&user.id)
    .bind(&cred.credential_id)
    .bind(&cred.public_key)
    .bind(&cred.rp_id)
    .bind(cred.sign_count as i64)
    .bind(name)
    .bind(req.enc_master_key.as_ref().map(|k| &k.0))
    .bind(t)
    .execute(&state.db)
    .await;
    match res {
        Err(e) if is_unique_violation(&e) => {
            return Err(AppError::Conflict("that passkey is already added".into()));
        }
        r => r?,
    };
    tracing::info!(username = %user.username, "passkey added");
    Ok((
        StatusCode::CREATED,
        Json(Passkey {
            id,
            name: name.into(),
            credential_id: B64(cred.credential_id),
            unlock: req.enc_master_key.is_some(),
            created_at: t,
            last_used_at: None,
        }),
    ))
}

pub async fn delete(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<String>,
    Json(req): Json<CurrentPassword>,
) -> Result<StatusCode> {
    if user.app_password_id.is_some() {
        return Err(AppError::Forbidden);
    }
    verify_current(&state, &user, &req.current_auth_key).await?;
    let r = sqlx::query("DELETE FROM passkeys WHERE id = ? AND user_id = ?")
        .bind(&id)
        .bind(&user.id)
        .execute(&state.db)
        .await?;
    if r.rows_affected() == 0 {
        return Err(AppError::NotFound);
    }
    Ok(StatusCode::NO_CONTENT)
}

// ---------------------------------------------------------------------------
// Signing in with a passkey alone. The challenge is stateless (an expiry and
// random bytes under the server's HMAC), so asking for one writes nothing;
// a used one is remembered until it expires, so it works once.
// ---------------------------------------------------------------------------

const CHALLENGE_LABEL: &[u8] = b"passkey-login\0";

/// expiry (8 bytes) || random (16) || HMAC (first 16 bytes).
fn challenge_expiry(state: &AppState, c: &[u8]) -> Option<i64> {
    if c.len() != 40 {
        return None;
    }
    let expires = i64::from_be_bytes(c[..8].try_into().unwrap());
    let msg = [CHALLENGE_LABEL, &c[..24]].concat();
    (hmac_verify_prefix(&state.secret[..], &msg, &c[24..]) && expires > now()).then_some(expires)
}

pub async fn login_options(State(state): State<AppState>) -> Result<Json<PasskeyRequest>> {
    let mut body = (now() + CHALLENGE_SECS).to_be_bytes().to_vec();
    body.extend(thencloud_crypto::random_bytes(16));
    let mac = hmac(&state.secret[..], &[CHALLENGE_LABEL, &body].concat());
    body.extend_from_slice(&mac[..16]);
    Ok(Json(PasskeyRequest {
        challenge: B64(body),
        allow_credentials: Vec::new(),
    }))
}

pub async fn login(
    State(state): State<AppState>,
    ip: ClientIp,
    Json(req): Json<PasskeyLoginRequest>,
) -> Result<Json<PasskeyLoginResponse>> {
    let ikey = ip.key().map(|k| format!("passkey-ip:{k}"));
    if ikey.as_deref().is_some_and(|k| state.limiter.blocked(k)) {
        return Err(AppError::RateLimited);
    }
    let c = &req.challenge.0;
    let expires = challenge_expiry(&state, c).ok_or(AppError::SignInExpired)?;
    let (user_id, enc_master_key) = match check_assertion(&state, "", &req.assertion, c, true).await
    {
        Ok(r) => r,
        Err(e) => {
            if let Some(k) = &ikey {
                state.limiter.fail(k);
            }
            return Err(e);
        }
    };
    // Each challenge signs in once.
    let used = sqlx::query(
        "INSERT INTO auth_challenges (id, purpose, challenge, expires_at) VALUES (?, 'used', ?, ?)",
    )
    .bind(thencloud_crypto::b64_encode(&sha256(c)))
    .bind(c)
    .bind(expires)
    .execute(&state.db)
    .await;
    match used {
        Err(e) if is_unique_violation(&e) => return Err(AppError::SignInExpired),
        r => r?,
    };
    let enc_master_key = enc_master_key.ok_or_else(|| {
        AppError::bad("this passkey can only confirm a sign-in; use your password first")
    })?;
    let user = user_by_id(&state, &user_id).await?;
    if user.disabled_at.is_some() {
        return Err(AppError::AccountDisabled);
    }
    let token = create_session(&state, &user.id, req.device_name.as_deref(), None).await?;
    Ok(Json(PasskeyLoginResponse {
        token,
        me: user.into_me(&state.config)?,
        credential_id: req.assertion.credential_id,
        enc_master_key: B64(enc_master_key),
    }))
}
