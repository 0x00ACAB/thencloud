//! Periodic cleanup of expired uploads, sessions and links, and thinning
//! of old file versions.

use std::time::Duration;

use crate::AppState;
use crate::error::Result;
use crate::routes::{trash, uploads, versions};
use crate::util::now;

pub fn spawn(state: AppState) {
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(Duration::from_secs(300));
        loop {
            tick.tick().await;
            if let Err(e) = run_once(&state).await {
                tracing::warn!(error = %e, "janitor run failed");
            }
        }
    });
}

pub async fn run_once(state: &AppState) -> Result<()> {
    let t = now();
    let expired: Vec<String> = sqlx::query_scalar("SELECT id FROM uploads WHERE expires_at <= ?")
        .bind(t)
        .fetch_all(&state.db)
        .await?;
    for id in &expired {
        uploads::discard(state, id).await?;
    }
    let sessions = sqlx::query("DELETE FROM sessions WHERE expires_at <= ?")
        .bind(t)
        .execute(&state.db)
        .await?;
    let gone: Vec<String> = sqlx::query_scalar(
        "SELECT id FROM public_links WHERE expires_at IS NOT NULL AND expires_at <= ?",
    )
    .bind(t)
    .fetch_all(&state.db)
    .await?;
    uploads::discard_link_uploads(state, &gone).await?;
    let links =
        sqlx::query("DELETE FROM public_links WHERE expires_at IS NOT NULL AND expires_at <= ?")
            .bind(t)
            .execute(&state.db)
            .await?;
    sqlx::query("DELETE FROM auth_challenges WHERE expires_at <= ?")
        .bind(t)
        .execute(&state.db)
        .await?;
    let trashed = trash::purge_expired(state, state.config.trash_days).await?;
    let thinned = if state.config.version_thinning {
        versions::thin_all(state).await?
    } else {
        0
    };
    state.limiter.prune();
    if !expired.is_empty()
        || sessions.rows_affected() > 0
        || links.rows_affected() > 0
        || trashed > 0
        || thinned > 0
    {
        tracing::info!(
            uploads = expired.len(),
            sessions = sessions.rows_affected(),
            links = links.rows_affected(),
            trashed,
            thinned,
            "janitor cleaned up"
        );
    }
    Ok(())
}
