//! File version history.
//!
//! Every finished upload to an existing file adds a version; the node points
//! at its current one. Old versions are kept up to `--max-versions` per file
//! and count toward the owner's quota. Each version's content key is wrapped
//! under the file's node key and bound to (node id, version id), so the
//! client can decrypt any of them without re-encryption.

use crate::routes::activity::{self, Event};
use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::Response;
use sqlx::{Sqlite, Transaction};
use thencloud_crypto::api::*;

use crate::AppState;
use crate::access::{self, Access};
use crate::auth::AuthUser;
use crate::db::{NodeRow, get_node};
use crate::error::{AppError, Result};
use crate::routes::nodes::chunk_response;
use crate::util::*;

#[derive(sqlx::FromRow)]
struct VersionRow {
    id: String,
    enc_content_key: Vec<u8>,
    enc_metadata: Vec<u8>,
    chunk_count: i64,
    size: i64,
    created_at: i64,
    created_by: String,
}

async fn file_node(state: &AppState, user: &AuthUser, id: &str, min: Access) -> Result<NodeRow> {
    access::require(&state.db, &user.id, id, min).await?;
    let node = get_node(&state.db, id).await?.ok_or(AppError::NotFound)?;
    if node.is_folder() {
        return Err(AppError::bad("folders don't have versions"));
    }
    Ok(node)
}

async fn version_of(state: &AppState, node_id: &str, version_id: &str) -> Result<VersionRow> {
    sqlx::query_as(
        "SELECT v.id, v.enc_content_key, v.enc_metadata, v.chunk_count, v.size, v.created_at, \
         COALESCE(u.username, '') AS created_by \
         FROM file_versions v LEFT JOIN users u ON u.id = v.created_by \
         WHERE v.id = ? AND v.node_id = ?",
    )
    .bind(version_id)
    .bind(node_id)
    .fetch_optional(&state.db)
    .await?
    .ok_or(AppError::NotFound)
}

pub async fn list(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<String>,
) -> Result<Json<Vec<FileVersion>>> {
    let node = file_node(&state, &user, &id, Access::Read).await?;
    let rows: Vec<VersionRow> = sqlx::query_as(
        "SELECT v.id, v.enc_content_key, v.enc_metadata, v.chunk_count, v.size, v.created_at, \
         COALESCE(u.username, '') AS created_by \
         FROM file_versions v LEFT JOIN users u ON u.id = v.created_by \
         WHERE v.node_id = ? ORDER BY v.created_at DESC, v.rowid DESC",
    )
    .bind(&id)
    .fetch_all(&state.db)
    .await?;
    Ok(Json(
        rows.into_iter()
            .map(|r| FileVersion {
                current: node.v_id.as_deref() == Some(r.id.as_str()),
                id: r.id,
                enc_content_key: B64(r.enc_content_key),
                enc_metadata: B64(r.enc_metadata),
                chunk_count: r.chunk_count as u32,
                size: r.size,
                created_at: r.created_at,
                created_by: r.created_by,
            })
            .collect(),
    ))
}

pub async fn chunk(
    State(state): State<AppState>,
    user: AuthUser,
    Path((id, version_id, idx)): Path<(String, String, u32)>,
) -> Result<Response> {
    file_node(&state, &user, &id, Access::Read).await?;
    let v = version_of(&state, &id, &version_id).await?;
    chunk_response(&state, &v.id, v.chunk_count, idx, &user.id).await
}

/// Make an older version current again. The previously current version
/// stays in the history, so a restore can itself be undone.
pub async fn restore(
    State(state): State<AppState>,
    user: AuthUser,
    Path((id, version_id)): Path<(String, String)>,
    Json(req): Json<RestoreVersionRequest>,
) -> Result<Json<Node>> {
    check_metadata(&req.enc_metadata)?;
    let node = file_node(&state, &user, &id, Access::Write).await?;
    version_of(&state, &id, &version_id).await?;
    if node.v_id.as_deref() == Some(version_id.as_str()) {
        return Err(AppError::bad("that version is already the current one"));
    }
    let res = sqlx::query(
        "UPDATE nodes SET current_version_id = ?, enc_metadata = ?, revision = revision + 1, \
         updated_at = ? WHERE id = ? AND revision = ?",
    )
    .bind(&version_id)
    .bind(&req.enc_metadata.0)
    .bind(coarse_now())
    .bind(&id)
    .bind(req.if_revision.unwrap_or(node.revision))
    .execute(&state.db)
    .await?;
    if res.rows_affected() == 0 {
        return Err(AppError::Conflict(
            "the file was modified by someone else; reload and retry".into(),
        ));
    }
    activity::note(&state, &user.id, &id, Event::Changed, None).await;
    Ok(Json(
        get_node(&state.db, &id)
            .await?
            .ok_or(AppError::NotFound)?
            .into_api(),
    ))
}

pub async fn delete(
    State(state): State<AppState>,
    user: AuthUser,
    Path((id, version_id)): Path<(String, String)>,
) -> Result<StatusCode> {
    let node = file_node(&state, &user, &id, Access::Write).await?;
    let v = version_of(&state, &id, &version_id).await?;
    if node.v_id.as_deref() == Some(v.id.as_str()) {
        return Err(AppError::bad(
            "the current version can't be deleted; delete the file instead",
        ));
    }
    let mut tx = state.db.begin().await?;
    remove_versions(&mut tx, &node.owner_id, &[(v.id.clone(), v.size)]).await?;
    tx.commit().await?;
    state.blobs.delete_version(&v.id).await;
    Ok(StatusCode::NO_CONTENT)
}

/// Delete version rows and release their quota. The caller deletes the
/// blobs after the transaction commits.
pub async fn remove_versions(
    tx: &mut Transaction<'_, Sqlite>,
    owner_id: &str,
    versions: &[(String, i64)],
) -> Result<()> {
    for (id, _) in versions {
        sqlx::query("DELETE FROM file_versions WHERE id = ?")
            .bind(id)
            .execute(&mut **tx)
            .await?;
    }
    let freed: i64 = versions.iter().map(|v| v.1).sum();
    sqlx::query("UPDATE users SET used_bytes = MAX(0, used_bytes - ?) WHERE id = ?")
        .bind(freed)
        .bind(owner_id)
        .execute(&mut **tx)
        .await?;
    Ok(())
}

/// Versions of a file beyond the newest `keep`, never including the
/// current one: `(id, size)`, oldest last.
pub async fn excess_versions(
    tx: &mut Transaction<'_, Sqlite>,
    node_id: &str,
    keep: i64,
) -> Result<Vec<(String, i64)>> {
    Ok(sqlx::query_as(
        "SELECT v.id, v.size FROM file_versions v JOIN nodes n ON n.id = v.node_id \
         WHERE v.node_id = ? AND v.id IS NOT n.current_version_id \
         ORDER BY v.created_at DESC, v.rowid DESC LIMIT -1 OFFSET ?",
    )
    .bind(node_id)
    .bind((keep - 1).max(0))
    .fetch_all(&mut **tx)
    .await?)
}

/// Free at least `needed` bytes for `owner_id` by deleting their oldest
/// non-current versions. Returns how much was freed.
pub async fn prune_for_space(state: &AppState, owner_id: &str, needed: i64) -> Result<i64> {
    let candidates: Vec<(String, i64)> = sqlx::query_as(
        "SELECT v.id, v.size FROM file_versions v JOIN nodes n ON n.id = v.node_id \
         WHERE n.owner_id = ? AND v.id IS NOT n.current_version_id \
         ORDER BY v.created_at, v.rowid",
    )
    .bind(owner_id)
    .fetch_all(&state.db)
    .await?;
    let mut picked = Vec::new();
    let mut freed = 0;
    for c in candidates {
        if freed >= needed {
            break;
        }
        freed += c.1;
        picked.push(c);
    }
    if picked.is_empty() {
        return Ok(0);
    }
    let mut tx = state.db.begin().await?;
    remove_versions(&mut tx, owner_id, &picked).await?;
    tx.commit().await?;
    for (id, _) in &picked {
        state.blobs.delete_version(id).await;
    }
    tracing::info!(
        owner_id,
        versions = picked.len(),
        freed,
        "pruned old versions to make room"
    );
    Ok(freed)
}

/// Old versions to drop under age-based thinning: all are kept for the
/// first hour, then one per hour for a day, one per day for 30 days and one
/// per week after that (the newest in each slot). `versions` is
/// `(id, created_at)` of one file's non-current versions.
pub fn thin(versions: &[(String, i64)], t: i64) -> Vec<String> {
    const HOUR: i64 = 3600;
    const DAY: i64 = 24 * HOUR;
    let mut sorted: Vec<&(String, i64)> = versions.iter().collect();
    sorted.sort_by_key(|v| std::cmp::Reverse(v.1));
    let mut seen = std::collections::HashSet::new();
    let mut drop = Vec::new();
    for (id, at) in sorted {
        let age = t - at;
        let slot = match age {
            a if a < HOUR => continue,
            a if a < DAY => (1, at.div_euclid(HOUR)),
            a if a < 30 * DAY => (2, at.div_euclid(DAY)),
            _ => (3, at.div_euclid(7 * DAY)),
        };
        if !seen.insert(slot) {
            drop.push(id.clone());
        }
    }
    drop
}

/// Apply `thin` to every file's history. Returns how many versions went.
pub async fn thin_all(state: &AppState) -> Result<usize> {
    let rows: Vec<(String, String, String, i64, i64)> = sqlx::query_as(
        // Versions from the last hour are all kept, and a file needs at
        // least two older ones for any to go.
        "SELECT v.id, v.node_id, n.owner_id, v.created_at, v.size FROM file_versions v \
         JOIN nodes n ON n.id = v.node_id WHERE v.id IS NOT n.current_version_id AND v.created_at < ?1 \
         AND v.node_id IN (SELECT node_id FROM file_versions WHERE created_at < ?1 \
                           GROUP BY node_id HAVING COUNT(*) > 1) \
         ORDER BY v.node_id",
    )
    .bind(now() - 3600)
    .fetch_all(&state.db)
    .await?;
    let t = now();
    let mut removed = 0;
    for file in rows.chunk_by(|a, b| a.1 == b.1) {
        let list: Vec<(String, i64)> = file.iter().map(|r| (r.0.clone(), r.3)).collect();
        let drop: std::collections::HashSet<String> = thin(&list, t).into_iter().collect();
        if drop.is_empty() {
            continue;
        }
        let picked: Vec<(String, i64)> = file
            .iter()
            .filter(|r| drop.contains(&r.0))
            .map(|r| (r.0.clone(), r.4))
            .collect();
        let mut tx = state.db.begin().await?;
        remove_versions(&mut tx, &file[0].2, &picked).await?;
        tx.commit().await?;
        for (id, _) in &picked {
            state.blobs.delete_version(id).await;
        }
        removed += picked.len();
    }
    Ok(removed)
}

#[cfg(test)]
mod tests {
    use super::thin;

    #[test]
    fn thinning_keeps_one_per_slot() {
        let h = 3600;
        let t = 1_000 * 24 * h;
        let v = |id: &str, age: i64| (id.to_string(), t - age);
        let list = vec![
            v("fresh1", 60),
            v("fresh2", 120),
            v("hour_a", 3 * h + 10),
            v("hour_b", 3 * h + 20),
            v("day_a", 3 * 24 * h + 10),
            v("day_b", 3 * 24 * h + 20),
            v("week_a", 60 * 24 * h + 10),
            v("week_b", 60 * 24 * h + 20),
        ];
        let mut d = thin(&list, t);
        d.sort();
        assert_eq!(d, ["day_b", "hour_b", "week_b"]);
    }
}
