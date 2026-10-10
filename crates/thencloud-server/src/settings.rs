//! Settings an admin can change while the server runs, stored in the
//! `settings` table. Anything not set there falls back to the command line.

use thencloud_crypto::api::{DownloaderAccess, Registration};

use crate::AppState;
use crate::error::Result;

async fn get(state: &AppState, key: &str) -> Result<Option<String>> {
    Ok(
        sqlx::query_scalar("SELECT value FROM settings WHERE key = ?")
            .bind(key)
            .fetch_optional(&state.db)
            .await?,
    )
}

async fn set(state: &AppState, key: &str, value: &str) -> Result<()> {
    sqlx::query("INSERT INTO settings (key, value) VALUES (?, ?) ON CONFLICT(key) DO UPDATE SET value = excluded.value")
        .bind(key)
        .bind(value)
        .execute(&state.db)
        .await?;
    Ok(())
}

/// Who may register. Defaults to `--allow-registration` (open, or closed).
pub async fn registration(state: &AppState) -> Result<Registration> {
    Ok(match get(state, "registration").await?.as_deref() {
        Some("open") => Registration::Open,
        Some("invite") => Registration::Invite,
        Some("closed") => Registration::Closed,
        _ if state.config.allow_registration => Registration::Open,
        _ => Registration::Closed,
    })
}

/// At start: say so when anyone who finds the server can make an account.
/// Their files are encrypted, so whoever runs it can't see what they store.
pub async fn warn_if_open(state: &AppState) -> Result<()> {
    let users: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM users")
        .fetch_one(&state.db)
        .await?;
    if users > 0 && registration(state).await? == Registration::Open {
        tracing::warn!(
            quota_bytes = state.config.default_quota,
            bot_check = crate::turnstile::enabled(state),
            "registration is open: anyone who finds this server can make an account. \
             Switch to invite only in the Admin view, or set THENCLOUD_ALLOW_REGISTRATION=false"
        );
    }
    Ok(())
}

pub async fn set_registration(state: &AppState, mode: Registration) -> Result<()> {
    let v = match mode {
        Registration::Open => "open",
        Registration::Invite => "invite",
        Registration::Closed => "closed",
    };
    set(state, "registration", v).await
}

/// Who may use the video downloader. Off unless an admin turned it on.
pub async fn downloader(state: &AppState) -> Result<DownloaderAccess> {
    Ok(match get(state, "downloader").await?.as_deref() {
        Some("admins") => DownloaderAccess::Admins,
        Some("everyone") => DownloaderAccess::Everyone,
        _ => DownloaderAccess::Off,
    })
}

pub async fn set_downloader(state: &AppState, access: DownloaderAccess) -> Result<()> {
    let v = match access {
        DownloaderAccess::Off => "off",
        DownloaderAccess::Admins => "admins",
        DownloaderAccess::Everyone => "everyone",
    };
    set(state, "downloader", v).await
}

/// Whether people may link a new Google Drive. On unless an admin turned
/// it off (and only if the server has a Google app at all).
pub async fn google_drive(state: &AppState) -> Result<bool> {
    Ok(state.storage.google.is_some()
        && get(state, "google_drive").await?.as_deref() != Some("off"))
}

pub async fn set_google_drive(state: &AppState, on: bool) -> Result<()> {
    set(state, "google_drive", if on { "on" } else { "off" }).await
}
