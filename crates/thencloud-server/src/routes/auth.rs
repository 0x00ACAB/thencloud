use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use thencloud_crypto::api::*;
use thencloud_crypto::{KEY_LEN, KdfParams, SALT_LEN};

use crate::AppState;
use crate::auth::{AuthUser, ClientIp, create_session};
use crate::error::{AppError, Result, is_unique_violation};
use crate::routes::two_factor;
use crate::settings;
use crate::util::*;

#[derive(sqlx::FromRow)]
pub(crate) struct UserRow {
    pub(crate) id: String,
    username: String,
    auth_hash: String,
    kdf_salt: Vec<u8>,
    kdf_params: String,
    enc_master_key: Vec<u8>,
    public_key: Vec<u8>,
    enc_private_key: Vec<u8>,
    root_node_id: String,
    quota_bytes: i64,
    used_bytes: i64,
    is_admin: bool,
    pub(crate) disabled_at: Option<i64>,
    recovery_hash: Option<String>,
    enc_master_key_recovery: Option<Vec<u8>>,
    recovery_created_at: Option<i64>,
    pub(crate) totp_secret: Option<Vec<u8>>,
    totp_created_at: Option<i64>,
    pub(crate) totp_last_step: Option<i64>,
    pq_public_key: Option<Vec<u8>>,
    enc_pq_private_key: Option<Vec<u8>>,
}

impl UserRow {
    pub(crate) fn into_me(self, cfg: &crate::Config) -> Result<Me> {
        let kdf_params: KdfParams = serde_json::from_str(&self.kdf_params)
            .map_err(|e| AppError::Internal(e.to_string()))?;
        Ok(Me {
            user_id: self.id,
            username: self.username,
            is_admin: self.is_admin,
            quota_bytes: self.quota_bytes,
            used_bytes: self.used_bytes,
            keys: KeyBundle {
                kdf_salt: B64(self.kdf_salt),
                kdf_params,
                enc_master_key: B64(self.enc_master_key),
                public_key: B64(self.public_key),
                enc_private_key: B64(self.enc_private_key),
                root_node_id: self.root_node_id,
                pq_public_key: self.pq_public_key.map(B64),
                enc_pq_private_key: self.enc_pq_private_key.map(B64),
            },
            max_versions: cfg.max_versions,
            trash_days: cfg.trash_days,
            recovery_created_at: self.recovery_created_at,
            totp_created_at: self.totp_created_at,
        })
    }
}

const USER_SELECT: &str = "SELECT id, username, auth_hash, kdf_salt, kdf_params, enc_master_key, public_key, \
     enc_private_key, root_node_id, quota_bytes, used_bytes, is_admin, disabled_at, recovery_hash, \
     enc_master_key_recovery, recovery_created_at, totp_secret, totp_created_at, totp_last_step, pq_public_key, enc_pq_private_key FROM users";

async fn user_by_name(state: &AppState, username: &str) -> Result<Option<UserRow>> {
    let sql = format!("{USER_SELECT} WHERE username = ?");
    Ok(sqlx::query_as(&sql)
        .bind(username)
        .fetch_optional(&state.db)
        .await?)
}

pub(crate) async fn user_by_id(state: &AppState, id: &str) -> Result<UserRow> {
    let sql = format!("{USER_SELECT} WHERE id = ?");
    sqlx::query_as(&sql)
        .bind(id)
        .fetch_optional(&state.db)
        .await?
        .ok_or(AppError::Unauthorized)
}

fn check_kdf(salt: &[u8], params: &KdfParams) -> Result<()> {
    if !(SALT_LEN..=64).contains(&salt.len()) {
        return Err(AppError::bad("kdf_salt must be 16-64 bytes"));
    }
    if !params.is_acceptable() {
        return Err(AppError::bad("kdf_params are outside the accepted range"));
    }
    Ok(())
}

/// Returns the KDF salt and parameters for a user. Unknown users get a
/// stable fake salt so this endpoint can't be used to enumerate accounts.
pub async fn prelogin(
    State(state): State<AppState>,
    Json(req): Json<PreloginRequest>,
) -> Result<Json<PreloginResponse>> {
    let username = req.username.trim().to_lowercase();
    if let Some(u) = user_by_name(&state, &username).await? {
        let kdf_params =
            serde_json::from_str(&u.kdf_params).map_err(|e| AppError::Internal(e.to_string()))?;
        return Ok(Json(PreloginResponse {
            kdf_salt: B64(u.kdf_salt),
            kdf_params,
        }));
    }
    let fake = hmac(&state.secret[..], format!("prelogin:{username}").as_bytes());
    Ok(Json(PreloginResponse {
        kdf_salt: B64(fake[..SALT_LEN].to_vec()),
        kdf_params: KdfParams::default(),
    }))
}

pub async fn register(
    State(state): State<AppState>,
    Json(req): Json<RegisterRequest>,
) -> Result<(StatusCode, Json<SessionResponse>)> {
    let username = normalize_username(&req.username)?;
    let user_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM users")
        .fetch_one(&state.db)
        .await?;
    // The first account can always be created (it becomes the admin).
    let invite = if user_count == 0 {
        None
    } else {
        match (settings::registration(&state).await?, req.invite.as_deref()) {
            (Registration::Open, _) => None,
            (Registration::Invite, Some(token)) => Some(sha256(token.as_bytes())),
            (Registration::Invite, None) | (Registration::Closed, _) => {
                return Err(AppError::RegistrationClosed);
            }
        }
    };
    check_len(&req.auth_key, KEY_LEN, "auth_key")?;
    check_kdf(&req.kdf_salt, &req.kdf_params)?;
    check_len(&req.enc_master_key, WRAPPED_KEY_LEN, "enc_master_key")?;
    check_len(&req.public_key, 32, "public_key")?;
    check_len(&req.enc_private_key, WRAPPED_KEY_LEN, "enc_private_key")?;
    check_pq_key(&req.pq_public_key, &req.enc_pq_private_key)?;
    check_id(&req.root.id, "root.id")?;
    check_len(&req.root.enc_key, WRAPPED_KEY_LEN, "root.enc_key")?;
    check_metadata(&req.root.enc_metadata)?;

    let auth_hash = hash_secret(req.auth_key.0.clone()).await?;
    let user_id = new_uuid();
    let t = now();

    let mut tx = state.db.begin().await?;
    let res = sqlx::query(
        "INSERT INTO users (id, username, auth_hash, kdf_salt, kdf_params, enc_master_key, public_key, \
         enc_private_key, root_node_id, quota_bytes, is_admin, created_at, pq_public_key, \
         enc_pq_private_key) VALUES (?,?,?,?,?,?,?,?,?,?,?,?,?,?)",
    )
    .bind(&user_id)
    .bind(&username)
    .bind(&auth_hash)
    .bind(&req.kdf_salt.0)
    .bind(serde_json::to_string(&req.kdf_params).unwrap())
    .bind(&req.enc_master_key.0)
    .bind(&req.public_key.0)
    .bind(&req.enc_private_key.0)
    .bind(&req.root.id)
    .bind(state.config.default_quota)
    .bind(user_count == 0)
    .bind(t)
    .bind(req.pq_public_key.as_ref().map(|k| &k.0))
    .bind(req.enc_pq_private_key.as_ref().map(|k| &k.0))
    .execute(&mut *tx)
    .await;
    match res {
        Err(e) if is_unique_violation(&e) => {
            return Err(AppError::Conflict("username is taken".into()));
        }
        r => r?,
    };
    let res = sqlx::query(
        "INSERT INTO nodes (id, owner_id, created_by, parent_id, kind, enc_key, enc_metadata, created_at, updated_at) \
         VALUES (?, ?, ?, NULL, 'folder', ?, ?, ?, ?)",
    )
    .bind(&req.root.id)
    .bind(&user_id)
    .bind(&user_id)
    .bind(&req.root.enc_key.0)
    .bind(&req.root.enc_metadata.0)
    .bind(coarse_now())
    .bind(coarse_now())
    .execute(&mut *tx)
    .await;
    match res {
        Err(e) if is_unique_violation(&e) => {
            return Err(AppError::Conflict("root id already exists".into()));
        }
        r => r?,
    };
    // Use up the invite in the same transaction, so it works exactly once.
    if let Some(hash) = invite {
        let used = sqlx::query(
            "UPDATE invites SET used_by = ?, used_at = ? \
             WHERE token_hash = ? AND used_by IS NULL AND used_at IS NULL AND expires_at > ?",
        )
        .bind(&user_id)
        .bind(t)
        .bind(&hash)
        .bind(t)
        .execute(&mut *tx)
        .await?;
        if used.rows_affected() != 1 {
            return Err(AppError::InvalidInvite);
        }
    }
    tx.commit().await?;
    tracing::info!(%username, admin = user_count == 0, "user registered");

    let token = create_session(&state, &user_id, req.device_name.as_deref(), None).await?;
    let me = user_by_id(&state, &user_id).await?.into_me(&state.config)?;
    Ok((StatusCode::CREATED, Json(SessionResponse { token, me })))
}

pub async fn login(
    State(state): State<AppState>,
    ip: ClientIp,
    Json(req): Json<LoginRequest>,
) -> Result<Json<LoginResponse>> {
    let username = req.username.trim().to_lowercase();
    let (ukey, ikey) = (
        format!("login-user:{username}"),
        ip.key().map(|k| format!("login-ip:{k}")),
    );
    if state.limiter.blocked(&ukey) || ikey.as_deref().is_some_and(|k| state.limiter.blocked(k)) {
        return Err(AppError::RateLimited);
    }
    let user = user_by_name(&state, &username).await?;
    let hash = user
        .as_ref()
        .map(|u| u.auth_hash.clone())
        .unwrap_or_else(|| (*state.dummy_hash).clone());
    let ok = verify_secret(req.auth_key.0.clone(), hash).await?;
    let user = match user {
        Some(u) if ok => u,
        _ => {
            state.limiter.fail(&ukey);
            if let Some(k) = &ikey {
                state.limiter.fail(k);
            }
            return Err(AppError::InvalidCredentials);
        }
    };
    state.limiter.clear(&ukey);
    // Only said after a correct password, so it reveals nothing new.
    if user.disabled_at.is_some() {
        return Err(AppError::AccountDisabled);
    }
    if let Some(second_factor) =
        two_factor::login_challenge(&state, &user, req.device_name.as_deref()).await?
    {
        return Ok(Json(LoginResponse::SecondFactor { second_factor }));
    }
    let token = create_session(&state, &user.id, req.device_name.as_deref(), None).await?;
    Ok(Json(LoginResponse::Session(Box::new(SessionResponse {
        token,
        me: user.into_me(&state.config)?,
    }))))
}

pub async fn logout(State(state): State<AppState>, user: AuthUser) -> Result<StatusCode> {
    sqlx::query("DELETE FROM sessions WHERE token_hash = ?")
        .bind(&user.token_hash)
        .execute(&state.db)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn me(State(state): State<AppState>, user: AuthUser) -> Result<Json<Me>> {
    Ok(Json(
        user_by_id(&state, &user.id).await?.into_me(&state.config)?,
    ))
}

/// Re-wraps the master key under a new password. File keys are unaffected.
/// All other sessions are signed out.
pub async fn change_password(
    State(state): State<AppState>,
    user: AuthUser,
    Json(req): Json<ChangePasswordRequest>,
) -> Result<StatusCode> {
    let key = format!("password-user:{}", user.id);
    if state.limiter.blocked(&key) {
        return Err(AppError::RateLimited);
    }
    check_len(&req.new_auth_key, KEY_LEN, "new_auth_key")?;
    check_kdf(&req.new_kdf_salt, &req.new_kdf_params)?;
    check_len(
        &req.new_enc_master_key,
        WRAPPED_KEY_LEN,
        "new_enc_master_key",
    )?;
    let row = user_by_id(&state, &user.id).await?;
    if !verify_secret(req.current_auth_key.0.clone(), row.auth_hash).await? {
        state.limiter.fail(&key);
        return Err(AppError::InvalidCredentials);
    }
    let new_hash = hash_secret(req.new_auth_key.0.clone()).await?;
    let mut tx = state.db.begin().await?;
    sqlx::query("UPDATE users SET auth_hash = ?, kdf_salt = ?, kdf_params = ?, enc_master_key = ? WHERE id = ?")
        .bind(new_hash)
        .bind(&req.new_kdf_salt.0)
        .bind(serde_json::to_string(&req.new_kdf_params).unwrap())
        .bind(&req.new_enc_master_key.0)
        .bind(&user.id)
        .execute(&mut *tx)
        .await?;
    sqlx::query("DELETE FROM sessions WHERE user_id = ? AND token_hash != ?")
        .bind(&user.id)
        .bind(&user.token_hash)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}

/// Add the post-quantum key to an account made before them. Once only: a
/// key that's already set is never replaced.
pub async fn set_pq_key(
    State(state): State<AppState>,
    user: AuthUser,
    Json(req): Json<SetPqKeyRequest>,
) -> Result<Json<Me>> {
    check_pq_key(
        &Some(req.pq_public_key.clone()),
        &Some(req.enc_pq_private_key.clone()),
    )?;
    let r = sqlx::query(
        "UPDATE users SET pq_public_key = ?, enc_pq_private_key = ? WHERE id = ? AND pq_public_key IS NULL",
    )
    .bind(&req.pq_public_key.0)
    .bind(&req.enc_pq_private_key.0)
    .bind(&user.id)
    .execute(&state.db)
    .await?;
    if r.rows_affected() == 0 {
        return Err(AppError::Conflict(
            "this account already has a post-quantum key".into(),
        ));
    }
    Ok(Json(
        user_by_id(&state, &user.id).await?.into_me(&state.config)?,
    ))
}

/// Public: what the sign-in screen can offer.
pub async fn options(State(state): State<AppState>) -> Result<Json<AuthOptions>> {
    let users: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM users")
        .fetch_one(&state.db)
        .await?;
    let registration = if users == 0 {
        Registration::Open
    } else {
        settings::registration(&state).await?
    };
    Ok(Json(AuthOptions { registration }))
}

// ---------------------------------------------------------------------------
// Recovery keys. The server stores an Argon2 hash of the key's auth part and
// the master key wrapped under its KEK; it never sees the key or the KEK.
// ---------------------------------------------------------------------------

/// Check the current password for a change to the recovery key.
pub(crate) async fn verify_current(
    state: &AppState,
    user: &AuthUser,
    auth_key: &B64,
) -> Result<UserRow> {
    let key = format!("password-user:{}", user.id);
    if state.limiter.blocked(&key) {
        return Err(AppError::RateLimited);
    }
    let row = user_by_id(state, &user.id).await?;
    if !verify_secret(auth_key.0.clone(), row.auth_hash.clone()).await? {
        state.limiter.fail(&key);
        return Err(AppError::InvalidCredentials);
    }
    Ok(row)
}

/// Delete your own account and everything in it. Needs the password again
/// (not an app password), and the last admin can't leave others without one.
pub async fn delete_me(
    State(state): State<AppState>,
    user: AuthUser,
    Json(req): Json<DeleteAccountRequest>,
) -> Result<StatusCode> {
    if user.app_password_id.is_some() {
        return Err(AppError::Forbidden);
    }
    verify_current(&state, &user, &req.current_auth_key).await?;
    if user.is_admin {
        let (admins, others): (i64, i64) = sqlx::query_as(
            "SELECT COUNT(*) FILTER (WHERE is_admin AND disabled_at IS NULL AND id != ?1), \
             COUNT(*) FILTER (WHERE id != ?1) FROM users",
        )
        .bind(&user.id)
        .fetch_one(&state.db)
        .await?;
        if admins == 0 && others > 0 {
            return Err(AppError::bad(
                "you're the only admin; make someone else an admin first",
            ));
        }
    }
    crate::routes::admin::delete_account(&state, &user.id).await?;
    tracing::info!(user = %user.id, "account deleted by its owner");
    Ok(StatusCode::NO_CONTENT)
}

pub async fn set_recovery(
    State(state): State<AppState>,
    user: AuthUser,
    Json(req): Json<SetRecoveryRequest>,
) -> Result<Json<Me>> {
    check_len(&req.recovery_auth_key, KEY_LEN, "recovery_auth_key")?;
    check_len(
        &req.enc_master_key_recovery,
        WRAPPED_KEY_LEN,
        "enc_master_key_recovery",
    )?;
    verify_current(&state, &user, &req.current_auth_key).await?;
    let hash = hash_secret(req.recovery_auth_key.0.clone()).await?;
    sqlx::query(
        "UPDATE users SET recovery_hash = ?, enc_master_key_recovery = ?, recovery_created_at = ? WHERE id = ?",
    )
    .bind(hash)
    .bind(&req.enc_master_key_recovery.0)
    .bind(now())
    .bind(&user.id)
    .execute(&state.db)
    .await?;
    Ok(Json(
        user_by_id(&state, &user.id).await?.into_me(&state.config)?,
    ))
}

pub async fn remove_recovery(
    State(state): State<AppState>,
    user: AuthUser,
    Json(req): Json<RemoveRecoveryRequest>,
) -> Result<Json<Me>> {
    verify_current(&state, &user, &req.current_auth_key).await?;
    sqlx::query(
        "UPDATE users SET recovery_hash = NULL, enc_master_key_recovery = NULL, recovery_created_at = NULL WHERE id = ?",
    )
    .bind(&user.id)
    .execute(&state.db)
    .await?;
    Ok(Json(
        user_by_id(&state, &user.id).await?.into_me(&state.config)?,
    ))
}

/// Verify a recovery auth key. Unknown users and users without a recovery
/// key take the same time and give the same answer as a wrong key.
async fn verify_recovery(
    state: &AppState,
    ip: &ClientIp,
    username: &str,
    auth_key: &B64,
) -> Result<UserRow> {
    let username = username.trim().to_lowercase();
    let (ukey, ikey) = (
        format!("recovery-user:{username}"),
        ip.key().map(|k| format!("recovery-ip:{k}")),
    );
    if state.limiter.blocked(&ukey) || ikey.as_deref().is_some_and(|k| state.limiter.blocked(k)) {
        return Err(AppError::RateLimited);
    }
    let user = user_by_name(state, &username).await?;
    let hash = user
        .as_ref()
        .and_then(|u| u.recovery_hash.clone())
        .unwrap_or_else(|| (*state.dummy_hash).clone());
    let ok = verify_secret(auth_key.0.clone(), hash).await?;
    match user {
        Some(u) if ok && u.enc_master_key_recovery.is_some() => {
            state.limiter.clear(&ukey);
            if u.disabled_at.is_some() {
                return Err(AppError::AccountDisabled);
            }
            Ok(u)
        }
        _ => {
            state.limiter.fail(&ukey);
            if let Some(k) = &ikey {
                state.limiter.fail(k);
            }
            Err(AppError::InvalidCredentials)
        }
    }
}

pub async fn recovery_unlock(
    State(state): State<AppState>,
    ip: ClientIp,
    Json(req): Json<RecoveryUnlockRequest>,
) -> Result<Json<RecoveryUnlockResponse>> {
    let u = verify_recovery(&state, &ip, &req.username, &req.recovery_auth_key).await?;
    Ok(Json(RecoveryUnlockResponse {
        enc_master_key_recovery: B64(u.enc_master_key_recovery.unwrap_or_default()),
    }))
}

/// Set a new password using the recovery key. Every other session is signed
/// out; the recovery key stays valid.
pub async fn recovery_reset(
    State(state): State<AppState>,
    ip: ClientIp,
    Json(req): Json<RecoveryResetRequest>,
) -> Result<Json<SessionResponse>> {
    check_len(&req.new_auth_key, KEY_LEN, "new_auth_key")?;
    check_kdf(&req.new_kdf_salt, &req.new_kdf_params)?;
    check_len(
        &req.new_enc_master_key,
        WRAPPED_KEY_LEN,
        "new_enc_master_key",
    )?;
    let u = verify_recovery(&state, &ip, &req.username, &req.recovery_auth_key).await?;
    let new_hash = hash_secret(req.new_auth_key.0.clone()).await?;
    let mut tx = state.db.begin().await?;
    sqlx::query("UPDATE users SET auth_hash = ?, kdf_salt = ?, kdf_params = ?, enc_master_key = ? WHERE id = ?")
        .bind(new_hash)
        .bind(&req.new_kdf_salt.0)
        .bind(serde_json::to_string(&req.new_kdf_params).unwrap())
        .bind(&req.new_enc_master_key.0)
        .bind(&u.id)
        .execute(&mut *tx)
        .await?;
    sqlx::query("DELETE FROM sessions WHERE user_id = ?")
        .bind(&u.id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    tracing::info!(username = %u.username, "password reset with a recovery key");
    let token = create_session(&state, &u.id, req.device_name.as_deref(), None).await?;
    Ok(Json(SessionResponse {
        token,
        me: user_by_id(&state, &u.id).await?.into_me(&state.config)?,
    }))
}
