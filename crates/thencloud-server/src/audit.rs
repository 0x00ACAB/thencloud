//! The admin audit log: who changed a quota or a limit, gave or took admin
//! rights, disabled, enabled or deleted an account, changed registration or
//! the downloader, or made or removed an invite. Admin actions only, never
//! anything about content; listed to admins (`routes/admin.rs`) and pruned
//! after a year by the janitor.

use sqlx::{Executor, Sqlite};

use crate::AppState;
use crate::error::Result;
use crate::util::now;

const KEEP_SECS: i64 = 365 * 86400;

/// The actions, as stored and sent to the client.
pub mod action {
    pub const QUOTA: &str = "quota";
    pub const DOWNLOAD_LIMIT: &str = "download_limit";
    pub const UPLOAD_LIMIT: &str = "upload_limit";
    pub const ADMIN_GRANTED: &str = "admin_granted";
    pub const ADMIN_REMOVED: &str = "admin_removed";
    pub const DISABLED: &str = "disabled";
    pub const ENABLED: &str = "enabled";
    pub const DELETED: &str = "deleted";
    pub const REGISTRATION: &str = "registration";
    pub const DOWNLOADER: &str = "downloader";
    pub const GOOGLE_DRIVE: &str = "google_drive";
    pub const INVITE_CREATED: &str = "invite_created";
    pub const INVITE_DELETED: &str = "invite_deleted";
}

/// Note one action by `actor` (a username), in the caller's transaction or not.
pub async fn record<'e, E: Executor<'e, Database = Sqlite>>(
    db: E,
    actor: &str,
    action: &str,
    target: Option<&str>,
    detail: Option<String>,
) -> Result<()> {
    sqlx::query(
        "INSERT INTO admin_audit (at, actor, action, target, detail) VALUES (?, ?, ?, ?, ?)",
    )
    .bind(now())
    .bind(actor)
    .bind(action)
    .bind(target)
    .bind(detail)
    .execute(db)
    .await?;
    Ok(())
}

pub async fn prune(state: &AppState) -> Result<u64> {
    Ok(sqlx::query("DELETE FROM admin_audit WHERE at < ?")
        .bind(now() - KEEP_SECS)
        .execute(&state.db)
        .await?
        .rows_affected())
}
