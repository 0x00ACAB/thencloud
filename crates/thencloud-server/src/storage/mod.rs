//! Storage people link to their own account (Google Drive first), on top of
//! the server's blob store (blob.rs).
//!
//! A linked account is either a **mirror**, which gets a copy of every
//! version kept on this server, or **extra space**, where new versions can be
//! kept instead of here. Each person also picks which comes first for new
//! files: this server, or their linked space (`users.storage_prefer`).
//!
//! What goes to a provider is the same ciphertext the server would keep,
//! chunk for chunk, under random names; `remote_chunks` says where each one
//! is. A version kept only at a provider names its account in
//! `file_versions.account_id` and doesn't count toward the server quota;
//! what thencloud keeps in an account is in `storage_accounts.used_bytes`.
//!
//! Mirrors are filled in the background (`run`), so a provider that's down
//! never holds up an upload; the same job retries deletes that failed and
//! asks providers how much room is left.

pub mod google;

use std::time::Duration;

use sqlx::SqlitePool;
use thencloud_crypto::{Key, MAX_ENCRYPTED_CHUNK};

use crate::AppState;
use crate::error::{AppError, Result};
use crate::util::now;
use google::{DriveError, Google};

pub const KEY_FILE: &str = "storage-token-key";

/// How often the background job runs, and how many chunks it copies to
/// each mirror per run.
const TICK: Duration = Duration::from_secs(60);
const MIRROR_BATCH: i64 = 64;
/// How long a provider's free space is trusted before asking again.
const FREE_SPACE_TTL: i64 = 3600;

/// The providers this server can talk to and the key their tokens are
/// sealed under.
pub struct Storage {
    key: Key,
    pub google: Option<Google>,
}

impl Storage {
    pub fn new(cfg: &crate::Config) -> std::io::Result<Storage> {
        Ok(Storage {
            key: crate::util::load_key_file(&cfg.data_dir.join(KEY_FILE))?,
            google: Google::from_config(cfg),
        })
    }

    pub fn seal(&self, account_id: &str, what: &str, value: &str) -> Vec<u8> {
        thencloud_crypto::seal_storage_secret(&self.key, account_id, what, value)
    }

    pub fn open(&self, account_id: &str, what: &str, sealed: &[u8]) -> Option<String> {
        thencloud_crypto::open_storage_secret(&self.key, account_id, what, sealed).ok()
    }
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct Account {
    pub id: String,
    pub user_id: String,
    pub provider: String,
    pub mode: String,
    pub enc_token: Vec<u8>,
    pub enc_label: Option<Vec<u8>>,
    pub folder_id: String,
    pub used_bytes: i64,
    pub free_bytes: Option<i64>,
    pub checked_at: Option<i64>,
    pub broken_at: Option<i64>,
    pub created_at: i64,
}

const ACCOUNT_SELECT: &str = "SELECT id, user_id, provider, mode, enc_token, enc_label, folder_id, \
     used_bytes, free_bytes, checked_at, broken_at, created_at FROM storage_accounts";

pub async fn account(db: &SqlitePool, id: &str) -> Result<Option<Account>> {
    Ok(sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "{ACCOUNT_SELECT} WHERE id = ?"
    )))
    .bind(id)
    .fetch_optional(db)
    .await?)
}

pub async fn accounts_of(db: &SqlitePool, user_id: &str) -> Result<Vec<Account>> {
    Ok(sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "{ACCOUNT_SELECT} WHERE user_id = ? ORDER BY created_at"
    )))
    .bind(user_id)
    .fetch_all(db)
    .await?)
}

/// What a provider's error means for whoever asked.
fn app_error(e: DriveError) -> AppError {
    match e {
        DriveError::Full => AppError::QuotaExceeded,
        DriveError::Unauthorized => AppError::Unavailable(
            "Google Drive no longer lets thencloud in; link it again in Settings".into(),
        ),
        DriveError::NotFound => AppError::Internal("a chunk is missing from Google Drive".into()),
        DriveError::Other(e) => {
            tracing::warn!(error = %e, "a linked storage account didn't answer");
            AppError::Unavailable("Google Drive didn't answer; try again in a moment".into())
        }
    }
}

/// The Google client, or an error if this server has none set up.
fn google(state: &AppState) -> Result<&Google> {
    state
        .storage
        .google
        .as_ref()
        .ok_or_else(|| AppError::Unavailable("this server can't reach Google Drive".into()))
}

/// An access token for the account. A refused refresh token marks the
/// account as needing to be linked again.
pub async fn access(state: &AppState, a: &Account) -> std::result::Result<String, DriveError> {
    let g = state
        .storage
        .google
        .as_ref()
        .ok_or_else(|| DriveError::Other("Google Drive isn't set up on this server".into()))?;
    let refresh = state
        .storage
        .open(&a.id, "refresh-token", &a.enc_token)
        .ok_or(DriveError::Unauthorized)?;
    match g.access_token(&a.id, &refresh).await {
        Err(DriveError::Unauthorized) => {
            mark_broken(state, &a.id).await;
            Err(DriveError::Unauthorized)
        }
        r => r,
    }
}

async fn mark_broken(state: &AppState, account_id: &str) {
    if let Some(g) = &state.storage.google {
        g.forget_access(account_id);
    }
    let _ =
        sqlx::query("UPDATE storage_accounts SET broken_at = ? WHERE id = ? AND broken_at IS NULL")
            .bind(now())
            .bind(account_id)
            .execute(&state.db)
            .await;
}

// ---------------------------------------------------------------------------
// Where new versions go
// ---------------------------------------------------------------------------

/// Where a new version of `owner_id`'s goes: None for this server, or an
/// extra-space account. `size` is an upper bound on its size.
pub async fn place_new(state: &AppState, owner_id: &str, size: i64) -> Result<Option<String>> {
    if state.storage.google.is_none() {
        return Ok(None);
    }
    let extras: Vec<(String, Option<i64>)> = sqlx::query_as(
        "SELECT id, free_bytes FROM storage_accounts \
         WHERE user_id = ? AND mode = 'extra' AND broken_at IS NULL ORDER BY created_at",
    )
    .bind(owner_id)
    .fetch_all(&state.db)
    .await?;
    let fits = extras
        .iter()
        .find(|(_, free)| free.is_none_or(|f| f >= size))
        .map(|(id, _)| id.clone());
    if fits.is_none() {
        return Ok(None);
    }
    let (prefer, used, quota): (String, i64, i64) =
        sqlx::query_as("SELECT storage_prefer, used_bytes, quota_bytes FROM users WHERE id = ?")
            .bind(owner_id)
            .fetch_one(&state.db)
            .await?;
    Ok(match prefer.as_str() {
        "linked" => fits,
        // This server first, the linked space once it's full.
        _ if used + size <= quota => None,
        _ => fits,
    })
}

/// The upper bound `place_new` gets for an upload of `chunk_count` chunks.
pub fn size_bound(chunk_count: u32) -> i64 {
    i64::from(chunk_count) * MAX_ENCRYPTED_CHUNK as i64
}

// ---------------------------------------------------------------------------
// Chunks
// ---------------------------------------------------------------------------

/// Store one chunk at a provider (an extra-space version, or a mirror's
/// copy). Replaces the chunk if it was sent before.
pub async fn put_remote(
    state: &AppState,
    account_id: &str,
    version_id: &str,
    idx: u32,
    data: &[u8],
) -> Result<()> {
    let a = account(&state.db, account_id)
        .await?
        .ok_or_else(|| AppError::Unavailable("that storage account was unlinked".into()))?;
    if a.broken_at.is_some() {
        return Err(app_error(DriveError::Unauthorized));
    }
    let token = access(state, &a).await.map_err(app_error)?;
    let g = google(state)?;
    let remote_id = g
        .upload(&token, &a.folder_id, data)
        .await
        .map_err(app_error)?;
    let old: Option<(String, i64)> = sqlx::query_as(
        "SELECT remote_id, size FROM remote_chunks WHERE account_id = ? AND version_id = ? AND idx = ?",
    )
    .bind(&a.id)
    .bind(version_id)
    .bind(idx)
    .fetch_optional(&state.db)
    .await?;
    let delta = data.len() as i64 - old.as_ref().map_or(0, |o| o.1);
    let mut tx = state.db.begin().await?;
    sqlx::query(
        "INSERT INTO remote_chunks (account_id, version_id, idx, remote_id, size) VALUES (?, ?, ?, ?, ?) \
         ON CONFLICT (account_id, version_id, idx) DO UPDATE SET remote_id = excluded.remote_id, size = excluded.size",
    )
    .bind(&a.id)
    .bind(version_id)
    .bind(idx)
    .bind(&remote_id)
    .bind(data.len() as i64)
    .execute(&mut *tx)
    .await?;
    adjust(&mut tx, &a.id, delta).await?;
    tx.commit().await?;
    if let Some((old_id, _)) = old {
        let _ = g.delete(&token, &old_id).await;
    }
    Ok(())
}

/// Change what thencloud keeps in an account (and the free space left).
async fn adjust(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    account_id: &str,
    delta: i64,
) -> Result<()> {
    sqlx::query(
        "UPDATE storage_accounts SET used_bytes = MAX(0, used_bytes + ?1), \
         free_bytes = CASE WHEN free_bytes IS NULL THEN NULL ELSE MAX(0, free_bytes - ?1) END WHERE id = ?2",
    )
    .bind(delta)
    .bind(account_id)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

/// One chunk of a stored version, from wherever it is: a provider for a
/// version kept there; otherwise this server, or a mirror if the server's
/// copy is missing.
pub async fn get_chunk(state: &AppState, version_id: &str, idx: u32) -> Result<Vec<u8>> {
    let home: Option<Option<String>> =
        sqlx::query_scalar("SELECT account_id FROM file_versions WHERE id = ?")
            .bind(version_id)
            .fetch_optional(&state.db)
            .await?;
    if let Some(Some(account_id)) = home {
        return get_remote(state, &account_id, version_id, idx).await;
    }
    match state.blobs.get_chunk(version_id, idx).await {
        Ok(data) => Ok(data),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            let mirrors: Vec<String> = sqlx::query_scalar(
                "SELECT account_id FROM remote_chunks WHERE version_id = ? AND idx = ?",
            )
            .bind(version_id)
            .bind(idx)
            .fetch_all(&state.db)
            .await?;
            for m in mirrors {
                if let Ok(data) = get_remote(state, &m, version_id, idx).await {
                    tracing::warn!(version_id, idx, "chunk missing here; served from a mirror");
                    return Ok(data);
                }
            }
            Err(AppError::Internal(format!(
                "blob missing for version {version_id} chunk {idx}"
            )))
        }
        Err(e) => Err(AppError::Io(e)),
    }
}

async fn get_remote(
    state: &AppState,
    account_id: &str,
    version_id: &str,
    idx: u32,
) -> Result<Vec<u8>> {
    let (remote_id, size): (String, i64) = sqlx::query_as(
        "SELECT remote_id, size FROM remote_chunks WHERE account_id = ? AND version_id = ? AND idx = ?",
    )
    .bind(account_id)
    .bind(version_id)
    .bind(idx)
    .fetch_optional(&state.db)
    .await?
    .ok_or_else(|| AppError::Internal(format!("no record of chunk {idx} of {version_id}")))?;
    let a = account(&state.db, account_id)
        .await?
        .ok_or_else(|| AppError::Internal("the account a chunk is in is gone".into()))?;
    let token = access(state, &a).await.map_err(app_error)?;
    let data = google(state)?
        .download(&token, &remote_id, MAX_ENCRYPTED_CHUNK as u64 + 64 * 1024)
        .await
        .map_err(app_error)?;
    // The chunk is decrypted (and so checked) by the client; a length that
    // differs from what was stored is caught here, where it can be told.
    if data.len() as i64 != size {
        return Err(AppError::Internal(format!(
            "chunk {idx} of {version_id} came back as {} bytes, not {size}",
            data.len()
        )));
    }
    Ok(data)
}

/// Delete versions' chunks everywhere: on this server and at providers. A
/// provider copy that can't be deleted now keeps its row, and `run` tries
/// again later.
pub async fn delete_versions(state: &AppState, version_ids: &[String]) {
    for v in version_ids {
        state.blobs.delete_version(v).await;
        let rows: Vec<(String, u32, String, i64)> = sqlx::query_as(
            "SELECT account_id, idx, remote_id, size FROM remote_chunks WHERE version_id = ?",
        )
        .bind(v)
        .fetch_all(&state.db)
        .await
        .unwrap_or_default();
        delete_remote(state, v, rows).await;
    }
}

async fn delete_remote(state: &AppState, version_id: &str, rows: Vec<(String, u32, String, i64)>) {
    let mut by_account: std::collections::HashMap<String, Vec<(u32, String, i64)>> =
        Default::default();
    for (a, idx, remote_id, size) in rows {
        by_account
            .entry(a)
            .or_default()
            .push((idx, remote_id, size));
    }
    for (account_id, chunks) in by_account {
        let Ok(Some(a)) = account(&state.db, &account_id).await else {
            // The account is gone: nothing left to delete there.
            let _ =
                sqlx::query("DELETE FROM remote_chunks WHERE account_id = ? AND version_id = ?")
                    .bind(&account_id)
                    .bind(version_id)
                    .execute(&state.db)
                    .await;
            continue;
        };
        let Ok(token) = access(state, &a).await else {
            continue;
        };
        let Some(g) = &state.storage.google else {
            continue;
        };
        for (idx, remote_id, size) in chunks {
            if g.delete(&token, &remote_id).await.is_err() {
                continue;
            }
            let done = async {
                let mut tx = state.db.begin().await?;
                sqlx::query(
                    "DELETE FROM remote_chunks WHERE account_id = ? AND version_id = ? AND idx = ?",
                )
                .bind(&a.id)
                .bind(version_id)
                .bind(idx)
                .execute(&mut *tx)
                .await?;
                adjust(&mut tx, &a.id, -size).await?;
                tx.commit().await?;
                Ok::<_, AppError>(())
            };
            let _ = done.await;
        }
    }
}

// ---------------------------------------------------------------------------
// Moving between places
// ---------------------------------------------------------------------------

/// How much one call to `move_batch` moves at most, so a request ends in
/// reasonable time.
const MOVE_BATCH_BYTES: i64 = 64 << 20;
const MOVE_BATCH_VERSIONS: usize = 32;

/// People whose files are being moved right now.
static MOVING: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());

struct MovingGuard(String);

impl MovingGuard {
    fn take(user_id: &str) -> Option<MovingGuard> {
        let mut m = MOVING.lock().unwrap_or_else(|e| e.into_inner());
        if m.iter().any(|u| u == user_id) {
            return None;
        }
        m.push(user_id.to_string());
        Some(MovingGuard(user_id.to_string()))
    }
}

impl Drop for MovingGuard {
    fn drop(&mut self) {
        MOVING
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .retain(|u| *u != self.0);
    }
}

/// What `move_batch` did.
#[derive(Debug, Default)]
pub struct Moved {
    pub versions: u32,
    pub bytes: i64,
    pub left: i64,
    pub left_bytes: i64,
    /// Some versions didn't fit at the destination.
    pub full: bool,
}

/// Move a batch of the account owner's versions: to this server
/// (`home`), or from it into the account (extra space only).
pub async fn move_batch(state: &AppState, a: &Account, home: bool) -> Result<Moved> {
    if !home && a.mode != "extra" {
        return Err(AppError::bad("files only move into extra space"));
    }
    if a.broken_at.is_some() {
        return Err(app_error(DriveError::Unauthorized));
    }
    // One move at a time per person: two moving the same version would each
    // clean up after the one that lost, and that can be the winner's copy.
    let _moving = MovingGuard::take(&a.user_id).ok_or_else(|| {
        AppError::Conflict("files are already being moved; try again when that's done".into())
    })?;
    let mut done = Moved::default();
    for (version_id, count, size) in movable(&state.db, a, home).await? {
        if done.versions as usize >= MOVE_BATCH_VERSIONS
            || (done.versions > 0 && done.bytes + size > MOVE_BATCH_BYTES)
        {
            break;
        }
        let r = if home {
            bring_home(state, a, &version_id, count as u32, size).await
        } else {
            send_away(state, a, &version_id, count as u32, size).await
        };
        match r {
            Ok(true) => {
                done.versions += 1;
                done.bytes += size;
            }
            Ok(false) => {}
            // Doesn't fit; a smaller one after it might.
            Err(AppError::QuotaExceeded) => done.full = true,
            Err(e) if done.versions > 0 => {
                tracing::warn!(error = %e, "moving a version stopped");
                break;
            }
            Err(e) => return Err(e),
        }
    }
    let (left, left_bytes) = left_to_move(&state.db, a, home).await?;
    done.left = left;
    done.left_bytes = left_bytes;
    Ok(done)
}

/// What's still to move, oldest first: `(version id, chunk count, size)`.
async fn movable(db: &SqlitePool, a: &Account, home: bool) -> Result<Vec<(String, i64, i64)>> {
    Ok(if home {
        sqlx::query_as(
            "SELECT id, chunk_count, size FROM file_versions WHERE account_id = ? \
             ORDER BY created_at, id LIMIT ?",
        )
        .bind(&a.id)
        .bind(MOVE_BATCH_VERSIONS as i64)
        .fetch_all(db)
        .await?
    } else {
        sqlx::query_as(
            "SELECT v.id, v.chunk_count, v.size FROM file_versions v JOIN nodes n ON n.id = v.node_id \
             WHERE n.owner_id = ? AND v.account_id IS NULL ORDER BY v.created_at, v.id LIMIT ?",
        )
        .bind(&a.user_id)
        .bind(MOVE_BATCH_VERSIONS as i64)
        .fetch_all(db)
        .await?
    })
}

async fn left_to_move(db: &SqlitePool, a: &Account, home: bool) -> Result<(i64, i64)> {
    Ok(if home {
        sqlx::query_as(
            "SELECT COUNT(*), COALESCE(SUM(size), 0) FROM file_versions WHERE account_id = ?",
        )
        .bind(&a.id)
        .fetch_one(db)
        .await?
    } else {
        sqlx::query_as(
            "SELECT COUNT(*), COALESCE(SUM(v.size), 0) FROM file_versions v \
             JOIN nodes n ON n.id = v.node_id WHERE n.owner_id = ? AND v.account_id IS NULL",
        )
        .bind(&a.user_id)
        .fetch_one(db)
        .await?
    })
}

/// Bytes of versions kept only in the account.
pub async fn only_there(db: &SqlitePool, account_id: &str) -> Result<i64> {
    Ok(
        sqlx::query_scalar("SELECT COALESCE(SUM(size), 0) FROM file_versions WHERE account_id = ?")
            .bind(account_id)
            .fetch_one(db)
            .await?,
    )
}

/// Copy one version from the account to this server, then make this
/// server its home. False if it was deleted or moved meanwhile. A mirror
/// keeps its chunks as its copy; extra space has them deleted.
async fn bring_home(
    state: &AppState,
    a: &Account,
    version_id: &str,
    count: u32,
    size: i64,
) -> Result<bool> {
    // Charged first, so two moves can't both fit in the same room.
    let charged = sqlx::query(
        "UPDATE users SET used_bytes = used_bytes + ?1 WHERE id = ?2 AND used_bytes + ?1 <= quota_bytes",
    )
    .bind(size)
    .bind(&a.user_id)
    .execute(&state.db)
    .await?;
    if charged.rows_affected() == 0 {
        return Err(AppError::QuotaExceeded);
    }
    let refund = || async {
        let _ = sqlx::query("UPDATE users SET used_bytes = MAX(0, used_bytes - ?) WHERE id = ?")
            .bind(size)
            .bind(&a.user_id)
            .execute(&state.db)
            .await;
        state.blobs.delete_version(version_id).await;
    };
    for idx in 0..count {
        let copied = async {
            let data = get_remote(state, &a.id, version_id, idx).await?;
            state.blobs.put_chunk(version_id, idx, &data).await?;
            Ok::<_, AppError>(())
        };
        if let Err(e) = copied.await {
            refund().await;
            return Err(e);
        }
    }
    let moved =
        sqlx::query("UPDATE file_versions SET account_id = NULL WHERE id = ? AND account_id = ?")
            .bind(version_id)
            .bind(&a.id)
            .execute(&state.db)
            .await?;
    if moved.rows_affected() == 0 {
        refund().await;
        return Ok(false);
    }
    if a.mode != "mirror" {
        let rows: Vec<(String, u32, String, i64)> = sqlx::query_as(
            "SELECT account_id, idx, remote_id, size FROM remote_chunks \
             WHERE account_id = ? AND version_id = ?",
        )
        .bind(&a.id)
        .bind(version_id)
        .fetch_all(&state.db)
        .await?;
        delete_remote(state, version_id, rows).await;
    }
    Ok(true)
}

/// Copy one version from this server to the account, then make the
/// account its home and free its room here. False if it was deleted or
/// moved meanwhile.
async fn send_away(
    state: &AppState,
    a: &Account,
    version_id: &str,
    count: u32,
    size: i64,
) -> Result<bool> {
    let free: Option<i64> =
        sqlx::query_scalar("SELECT free_bytes FROM storage_accounts WHERE id = ?")
            .bind(&a.id)
            .fetch_one(&state.db)
            .await?;
    if free.is_some_and(|f| f < size) {
        return Err(AppError::QuotaExceeded);
    }
    let have: Vec<u32> =
        sqlx::query_scalar("SELECT idx FROM remote_chunks WHERE account_id = ? AND version_id = ?")
            .bind(&a.id)
            .bind(version_id)
            .fetch_all(&state.db)
            .await?;
    for idx in (0..count).filter(|i| !have.contains(i)) {
        let data = get_chunk(state, version_id, idx).await?;
        put_remote(state, &a.id, version_id, idx, &data).await?;
    }
    let mut tx = state.db.begin().await?;
    let moved =
        sqlx::query("UPDATE file_versions SET account_id = ? WHERE id = ? AND account_id IS NULL")
            .bind(&a.id)
            .bind(version_id)
            .execute(&mut *tx)
            .await?;
    if moved.rows_affected() == 0 {
        drop(tx);
        // Deleted meanwhile: `run` clears the copies of versions that are
        // gone; moved elsewhere, they'd only be a spare copy, so clear them.
        let rows: Vec<(String, u32, String, i64)> = sqlx::query_as(
            "SELECT account_id, idx, remote_id, size FROM remote_chunks \
             WHERE account_id = ? AND version_id = ?",
        )
        .bind(&a.id)
        .bind(version_id)
        .fetch_all(&state.db)
        .await?;
        delete_remote(state, version_id, rows).await;
        return Ok(false);
    }
    sqlx::query("UPDATE users SET used_bytes = MAX(0, used_bytes - ?) WHERE id = ?")
        .bind(size)
        .bind(&a.user_id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    // A read that looked up the old home finds the chunk missing here and
    // falls back to the account's copy (`get_chunk`).
    state.blobs.delete_version(version_id).await;
    Ok(true)
}

// ---------------------------------------------------------------------------
// Accounts
// ---------------------------------------------------------------------------

/// Unlink an account: refused while versions are kept only there. Its
/// mirror copies are deleted (best effort) and its token given back.
pub async fn unlink(state: &AppState, a: &Account) -> Result<()> {
    // Not while files are moving: one could land there after the count.
    let _moving = MovingGuard::take(&a.user_id).ok_or_else(|| {
        AppError::Conflict("files are being moved; try again when that's done".into())
    })?;
    let only_there: i64 = sqlx::query_scalar(
        "SELECT (SELECT COUNT(*) FROM file_versions WHERE account_id = ?1) \
         + (SELECT COUNT(*) FROM uploads WHERE account_id = ?1)",
    )
    .bind(&a.id)
    .fetch_one(&state.db)
    .await?;
    if only_there > 0 {
        return Err(AppError::Conflict(
            "files are kept only in this account; they need to move back first".into(),
        ));
    }
    let rows: Vec<(String, u32, String, i64)> = sqlx::query_as(
        "SELECT version_id, idx, remote_id, size FROM remote_chunks WHERE account_id = ?",
    )
    .bind(&a.id)
    .fetch_all(&state.db)
    .await?;
    if let (Ok(token), Some(g)) = (access(state, a).await, &state.storage.google) {
        for (_, _, remote_id, _) in &rows {
            let _ = g.delete(&token, remote_id).await;
        }
        let _ = g.delete(&token, &a.folder_id).await;
    }
    if let (Some(g), Some(refresh)) = (
        &state.storage.google,
        state.storage.open(&a.id, "refresh-token", &a.enc_token),
    ) {
        g.revoke(&refresh).await;
        g.forget_access(&a.id);
    }
    let mut tx = state.db.begin().await?;
    sqlx::query("DELETE FROM remote_chunks WHERE account_id = ?")
        .bind(&a.id)
        .execute(&mut *tx)
        .await?;
    sqlx::query("DELETE FROM storage_accounts WHERE id = ?")
        .bind(&a.id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(())
}

/// Before an account is deleted: unlink everything it linked. Its versions
/// are already gone by then, so nothing is kept only there.
pub async fn forget_user(state: &AppState, user_id: &str) -> Result<()> {
    for a in accounts_of(&state.db, user_id).await? {
        unlink(state, &a).await?;
    }
    Ok(())
}

/// Ask the provider how much room is left, if it's been a while.
pub async fn refresh_free(state: &AppState, a: &Account, force: bool) {
    if !force && a.checked_at.is_some_and(|t| now() - t < FREE_SPACE_TTL) {
        return;
    }
    let (Ok(token), Some(g)) = (access(state, a).await, &state.storage.google) else {
        return;
    };
    if let Ok(about) = g.about(&token).await {
        let _ =
            sqlx::query("UPDATE storage_accounts SET free_bytes = ?, checked_at = ? WHERE id = ?")
                .bind(about.free)
                .bind(now())
                .bind(&a.id)
                .execute(&state.db)
                .await;
    }
}

// ---------------------------------------------------------------------------
// Checking
// ---------------------------------------------------------------------------

/// What `check` found in one linked account.
#[derive(Debug, Default)]
pub struct AccountCheck {
    pub account_id: String,
    pub user_id: String,
    pub mode: String,
    /// Why the account couldn't be listed, if it couldn't.
    pub unreachable: Option<String>,
    /// Chunks the database has there.
    pub chunks: u64,
    /// `(version id, index)` of chunks recorded there but not found.
    pub missing: Vec<(String, u32)>,
    /// `(version id/index, expected, found)`.
    pub wrong_size: Vec<(String, u64, u64)>,
    /// Versions kept only there that lack a record of some chunk.
    pub incomplete: Vec<String>,
    /// Files in thencloud's folder there that nothing refers to.
    pub unknown: u64,
}

impl AccountCheck {
    pub fn is_ok(&self) -> bool {
        self.unreachable.is_none()
            && self.missing.is_empty()
            && self.wrong_size.is_empty()
            && self.incomplete.is_empty()
    }
}

/// Check every linked account's chunks against what the provider lists.
pub async fn check(state: &AppState) -> Result<Vec<AccountCheck>> {
    let accounts: Vec<Account> = sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "{ACCOUNT_SELECT} ORDER BY user_id, created_at"
    )))
    .fetch_all(&state.db)
    .await?;
    let mut out = Vec::new();
    for a in accounts {
        let mut c = AccountCheck {
            account_id: a.id.clone(),
            user_id: a.user_id.clone(),
            mode: a.mode.clone(),
            ..Default::default()
        };
        c.incomplete = sqlx::query_scalar(
            "SELECT v.id FROM file_versions v WHERE v.account_id = ?1 \
             AND (SELECT COUNT(*) FROM remote_chunks r WHERE r.account_id = ?1 AND r.version_id = v.id) \
             < v.chunk_count ORDER BY v.id",
        )
        .bind(&a.id)
        .fetch_all(&state.db)
        .await?;
        let rows: Vec<(String, u32, String, i64)> = sqlx::query_as(
            "SELECT version_id, idx, remote_id, size FROM remote_chunks WHERE account_id = ? \
             ORDER BY version_id, idx",
        )
        .bind(&a.id)
        .fetch_all(&state.db)
        .await?;
        c.chunks = rows.len() as u64;
        let listed = async {
            let token = access(state, &a).await?;
            let g = state.storage.google.as_ref().ok_or_else(|| {
                DriveError::Other("Google Drive isn't set up on this server".into())
            })?;
            g.list(&token, &a.folder_id).await
        };
        let listed: std::collections::HashMap<String, u64> = match listed.await {
            Ok(l) => l.into_iter().collect(),
            Err(e) => {
                c.unreachable = Some(e.to_string());
                out.push(c);
                continue;
            }
        };
        let mut known = std::collections::HashSet::new();
        for (version, idx, remote_id, size) in rows {
            match listed.get(&remote_id) {
                None => c.missing.push((version, idx)),
                Some(&found) if found != size as u64 => {
                    c.wrong_size
                        .push((format!("{version}/{idx}"), size as u64, found))
                }
                Some(_) => {}
            }
            known.insert(remote_id);
        }
        c.unknown = listed.keys().filter(|id| !known.contains(*id)).count() as u64;
        out.push(c);
    }
    Ok(out)
}

// ---------------------------------------------------------------------------
// In the background
// ---------------------------------------------------------------------------

pub fn spawn(state: AppState) {
    if state.storage.google.is_none() {
        return;
    }
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(TICK);
        loop {
            tokio::select! {
                _ = tick.tick() => {}
                () = state.shutdown.cancelled() => return,
            }
            if let Err(e) = run(&state).await {
                tracing::warn!(error = %e, "linked storage run failed");
            }
        }
    });
}

/// One run: retry deletes that failed, fill mirrors, check free space.
pub async fn run(state: &AppState) -> Result<()> {
    // Copies of versions that no longer exist (a delete that failed, or
    // that was cut short).
    let orphans: Vec<String> = sqlx::query_scalar(
        "SELECT DISTINCT r.version_id FROM remote_chunks r \
         WHERE NOT EXISTS (SELECT 1 FROM file_versions v WHERE v.id = r.version_id) \
         AND NOT EXISTS (SELECT 1 FROM uploads u WHERE u.version_id = r.version_id) LIMIT 100",
    )
    .fetch_all(&state.db)
    .await?;
    for v in orphans {
        let rows: Vec<(String, u32, String, i64)> = sqlx::query_as(
            "SELECT account_id, idx, remote_id, size FROM remote_chunks WHERE version_id = ?",
        )
        .bind(&v)
        .fetch_all(&state.db)
        .await?;
        delete_remote(state, &v, rows).await;
    }
    let accounts: Vec<Account> = sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "{ACCOUNT_SELECT} WHERE broken_at IS NULL"
    )))
    .fetch_all(&state.db)
    .await?;
    for a in accounts {
        refresh_free(state, &a, false).await;
        if a.mode == "mirror" {
            fill_mirror(state, &a).await?;
        }
    }
    Ok(())
}

/// Copy up to `MIRROR_BATCH` chunks this server has and the mirror doesn't.
pub async fn fill_mirror(state: &AppState, a: &Account) -> Result<u32> {
    let missing: Vec<(String, i64)> = sqlx::query_as(
        "SELECT v.id, v.chunk_count FROM file_versions v JOIN nodes n ON n.id = v.node_id \
         WHERE n.owner_id = ?1 AND v.account_id IS NULL \
         AND (SELECT COUNT(*) FROM remote_chunks r WHERE r.account_id = ?2 AND r.version_id = v.id) < v.chunk_count \
         ORDER BY v.created_at LIMIT ?3",
    )
    .bind(&a.user_id)
    .bind(&a.id)
    .bind(MIRROR_BATCH)
    .fetch_all(&state.db)
    .await?;
    let mut copied = 0;
    for (version_id, count) in missing {
        let have: Vec<u32> = sqlx::query_scalar(
            "SELECT idx FROM remote_chunks WHERE account_id = ? AND version_id = ?",
        )
        .bind(&a.id)
        .bind(&version_id)
        .fetch_all(&state.db)
        .await?;
        for idx in (0..count as u32).filter(|i| !have.contains(i)) {
            if i64::from(copied) >= MIRROR_BATCH {
                return Ok(copied);
            }
            let Ok(data) = state.blobs.get_chunk(&version_id, idx).await else {
                continue;
            };
            match put_remote(state, &a.id, &version_id, idx, &data).await {
                Ok(()) => copied += 1,
                // Down, full or unlinked: try again next time.
                Err(_) => return Ok(copied),
            }
        }
    }
    Ok(copied)
}

/// For Settings: (bytes this server keeps for the user that a mirror should
/// have, bytes that mirror has of them).
pub async fn mirror_progress(db: &SqlitePool, a: &Account) -> Result<(i64, i64)> {
    Ok(sqlx::query_as(
        "SELECT \
           (SELECT COALESCE(SUM(v.size), 0) FROM file_versions v JOIN nodes n ON n.id = v.node_id \
             WHERE n.owner_id = ?1 AND v.account_id IS NULL), \
           (SELECT COALESCE(SUM(r.size), 0) FROM remote_chunks r JOIN file_versions v ON v.id = r.version_id \
             WHERE r.account_id = ?2 AND v.account_id IS NULL)",
    )
    .bind(&a.user_id)
    .bind(&a.id)
    .fetch_one(db)
    .await?)
}
