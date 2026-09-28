//! Activity in folders: who added, changed, renamed, moved, trashed or
//! restored what. The server sees these events anyway; they're kept for 90
//! days (see the janitor) and shown to anyone who can read the folder. Only
//! node ids are kept: the browser finds and decrypts the names, so the
//! server learns nothing it didn't already see.

use axum::Json;
use axum::extract::{Path, Query, State};
use serde::Deserialize;
use sqlx::SqlitePool;
use thencloud_crypto::api::ActivityEvent;

use crate::AppState;
use crate::access::{self, Access};
use crate::auth::AuthUser;
use crate::error::Result;
use crate::util::{coarse_now, now};

/// How long events are kept.
pub const KEEP_DAYS: i64 = 90;
const PAGE: i64 = 100;

#[derive(Clone, Copy)]
pub enum Event {
    Added,
    Changed,
    Renamed,
    Moved,
    Trashed,
    Restored,
}

impl Event {
    fn as_str(self) -> &'static str {
        match self {
            Event::Added => "added",
            Event::Changed => "changed",
            Event::Renamed => "renamed",
            Event::Moved => "moved",
            Event::Trashed => "trashed",
            Event::Restored => "restored",
        }
    }
}

/// Note an event. Best effort: a failure is logged, never passed on, since
/// what it records has already happened. `was_in` is the folder a moved
/// item came from, so the move shows there too.
pub async fn note(db: &SqlitePool, actor: &str, node_id: &str, what: Event, was_in: Option<&str>) {
    if let Err(e) = record(db, actor, node_id, what, was_in).await {
        tracing::warn!(error = %e, "couldn't record activity");
    }
}

async fn record(
    db: &SqlitePool,
    actor: &str,
    node_id: &str,
    what: Event,
    was_in: Option<&str>,
) -> Result<()> {
    let Some((parent, is_folder)): Option<(Option<String>, bool)> =
        sqlx::query_as("SELECT parent_id, kind = 'folder' FROM nodes WHERE id = ?")
            .bind(node_id)
            .fetch_optional(db)
            .await?
    else {
        return Ok(());
    };
    let at = coarse_now();
    // The same thing by the same person in the same hour is one event:
    // saving a note every few seconds shouldn't fill the history.
    let id: Option<i64> = sqlx::query_scalar(
        "INSERT INTO activity (actor_id, node_id, kind, is_folder, at) \
         SELECT ?, ?, ?, ?, ? WHERE NOT EXISTS (SELECT 1 FROM activity \
         WHERE node_id = ? AND actor_id = ? AND kind = ? AND at = ?) RETURNING id",
    )
    .bind(actor)
    .bind(node_id)
    .bind(what.as_str())
    .bind(is_folder)
    .bind(at)
    .bind(node_id)
    .bind(actor)
    .bind(what.as_str())
    .bind(at)
    .fetch_optional(db)
    .await?;
    let Some(id) = id else { return Ok(()) };
    for start in [parent.as_deref(), was_in].into_iter().flatten() {
        sqlx::query(
            "WITH RECURSIVE anc(id, parent_id) AS ( \
                SELECT id, parent_id FROM nodes WHERE id = ? \
                UNION ALL \
                SELECT n.id, n.parent_id FROM nodes n JOIN anc ON n.id = anc.parent_id \
             ) \
             INSERT OR IGNORE INTO activity_scope (folder_id, event_id) SELECT id, ? FROM anc",
        )
        .bind(start)
        .bind(id)
        .execute(db)
        .await?;
    }
    Ok(())
}

#[derive(Deserialize)]
pub struct Page {
    /// Events older than this one (for the next page).
    pub before: Option<i64>,
}

#[derive(sqlx::FromRow)]
struct Row {
    id: i64,
    node_id: String,
    actor: String,
    kind: String,
    is_folder: bool,
    at: i64,
}

/// A folder's activity, newest first, a page at a time.
pub async fn list(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<String>,
    Query(page): Query<Page>,
) -> Result<Json<Vec<ActivityEvent>>> {
    access::require(&state.db, &user.id, &id, Access::Read).await?;
    let rows: Vec<Row> = sqlx::query_as(
        "SELECT a.id, a.node_id, u.username AS actor, a.kind, a.is_folder, a.at \
         FROM activity_scope s JOIN activity a ON a.id = s.event_id JOIN users u ON u.id = a.actor_id \
         WHERE s.folder_id = ? AND (? IS NULL OR a.id < ?) ORDER BY a.id DESC LIMIT ?",
    )
    .bind(&id)
    .bind(page.before)
    .bind(page.before)
    .bind(PAGE)
    .fetch_all(&state.db)
    .await?;
    Ok(Json(
        rows.into_iter()
            .map(|r| ActivityEvent {
                id: r.id,
                node_id: r.node_id,
                actor: r.actor,
                kind: r.kind,
                folder: r.is_folder,
                at: r.at,
            })
            .collect(),
    ))
}

/// Drop events older than [`KEEP_DAYS`]. Returns how many went.
pub async fn prune(db: &SqlitePool) -> Result<u64> {
    let r = sqlx::query("DELETE FROM activity WHERE at < ?")
        .bind(now() - KEEP_DAYS * 86_400)
        .execute(db)
        .await?;
    Ok(r.rows_affected())
}
