//! File reports (#108). Someone who can open a file they don't own shows one
//! version of it to the admins: its content key, name, type and a note, sealed
//! to each admin in their browser (`seal_report`). The server never sees any
//! of it. While a report is open the reported version's blobs are kept, even
//! if the owner deletes the file, until an admin removes it or marks it safe.

use std::collections::HashSet;

use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use sqlx::SqlitePool;
use thencloud_crypto::api::*;
use thencloud_crypto::{REPORT_SEALED_HYBRID_LEN, REPORT_SEALED_LEN};

use crate::AppState;
use crate::access;
use crate::auth::AuthUser;
use crate::db::{NodeRow, get_node};
use crate::error::{AppError, Result};
use crate::util::{check_id, coarse_now};

/// Open reports made through a public link on one node before more from
/// links are refused. Signed-in reporters aren't capped this way: the
/// `reports_one_per_reporter` unique index already limits each of them to
/// one open report per file.
const MAX_OPEN_PER_NODE: i64 = 20;

pub enum Reporter<'a> {
    User(&'a str),
    /// A public link's visitor, rate limited by address (never stored).
    Link {
        ip: Option<String>,
    },
}

fn reason_str(r: ReportReason) -> &'static str {
    match r {
        ReportReason::Illegal => "illegal",
        ReportReason::Malware => "malware",
        ReportReason::Copyright => "copyright",
        ReportReason::Harassment => "harassment",
        ReportReason::Other => "other",
    }
}

/// Every admin's sealing key, for `seal_report`. Ids, not usernames.
pub async fn admin_keys(db: &SqlitePool) -> Result<Vec<ReportKey>> {
    let rows: Vec<(String, Vec<u8>, Option<Vec<u8>>)> = sqlx::query_as(
        "SELECT id, public_key, pq_public_key FROM users \
         WHERE is_admin = 1 AND disabled_at IS NULL ORDER BY id",
    )
    .fetch_all(db)
    .await?;
    Ok(rows
        .into_iter()
        .map(|(admin_id, pk, pq)| ReportKey {
            admin_id,
            public_key: B64(pk),
            pq_public_key: pq.map(B64),
        })
        .collect())
}

pub async fn keys(State(state): State<AppState>, _user: AuthUser) -> Result<Json<Vec<ReportKey>>> {
    Ok(Json(admin_keys(&state.db).await?))
}

/// Boxes must be report-sized and cover exactly the admins in `admins`. Each
/// box's size must match its admin's key: hybrid when they have an ML-KEM
/// key, classic otherwise (CLAUDE.md: never let a hybrid recipient be sealed
/// to in the weaker, X25519-only form).
fn check_boxes(boxes: &[ReportBox], admins: &[ReportKey]) -> Result<()> {
    let want: HashSet<&str> = admins.iter().map(|a| a.admin_id.as_str()).collect();
    let got: HashSet<&str> = boxes.iter().map(|b| b.admin_id.as_str()).collect();
    if want.is_empty() || got != want || got.len() != boxes.len() {
        return Err(AppError::bad("a report needs one box for each admin"));
    }
    for b in boxes {
        let admin = admins
            .iter()
            .find(|a| a.admin_id == b.admin_id)
            .expect("checked above: got == want");
        let want_len = if admin.pq_public_key.is_some() {
            REPORT_SEALED_HYBRID_LEN
        } else {
            REPORT_SEALED_LEN
        };
        if b.sealed.len() != want_len {
            return Err(AppError::bad("sealed has an invalid size"));
        }
    }
    Ok(())
}

pub async fn create_route(
    State(state): State<AppState>,
    user: AuthUser,
    Json(req): Json<CreateReportRequest>,
) -> Result<StatusCode> {
    check_id(&req.node_id, "node_id")?;
    // Not visible, trashed or not there: all look the same.
    if access::access(&state.db, &user.id, &req.node_id)
        .await?
        .is_none()
    {
        return Err(AppError::NotFound);
    }
    let node = get_node(&state.db, &req.node_id)
        .await?
        .ok_or(AppError::NotFound)?;
    if node.owner_id == user.id {
        return Err(AppError::Forbidden);
    }
    create(&state, Reporter::User(&user.id), &node, req).await
}

/// Store a report on `node` (already checked to be visible to the reporter).
pub async fn create(
    state: &AppState,
    reporter: Reporter<'_>,
    node: &NodeRow,
    req: CreateReportRequest,
) -> Result<StatusCode> {
    check_id(&req.id, "id")?;
    check_id(&req.version_id, "version_id")?;
    if node.is_folder() {
        return Err(AppError::bad("only files can be reported"));
    }
    let chunks: Option<i64> =
        sqlx::query_scalar("SELECT chunk_count FROM file_versions WHERE id = ? AND node_id = ?")
            .bind(&req.version_id)
            .bind(&node.id)
            .fetch_optional(&state.db)
            .await?;
    let chunks = chunks.ok_or_else(|| AppError::bad("not a version of this file"))?;
    check_boxes(&req.boxes, &admin_keys(&state.db).await?)?;

    let (reporter_id, via_link) = match &reporter {
        Reporter::User(id) => (Some(*id), false),
        Reporter::Link { ip } => {
            let key = format!("report:{}", ip.clone().unwrap_or_default());
            if state.limiter.blocked(&key) {
                return Err(AppError::TooManyReports);
            }
            state.limiter.fail(&key);
            (None, true)
        }
    };

    let mut tx = state.db.begin().await?;
    // Signed-in reporters are bounded by one open report per person per file
    // (the unique index below); this cap only guards against a flood of
    // anonymous link reports on one node.
    if via_link {
        let open: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM reports WHERE node_id = ? AND status = 'open' AND via_link = 1",
        )
        .bind(&node.id)
        .fetch_one(&mut *tx)
        .await?;
        if open >= MAX_OPEN_PER_NODE {
            return Err(AppError::TooManyReports);
        }
    }
    let r = sqlx::query(
        "INSERT INTO reports (id, node_id, version_id, chunk_count, owner_id, reason, reporter_id, via_link, created_at) \
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&req.id)
    .bind(&node.id)
    .bind(&req.version_id)
    .bind(chunks)
    .bind(&node.owner_id)
    .bind(reason_str(req.reason))
    .bind(reporter_id)
    .bind(via_link)
    .bind(coarse_now())
    .execute(&mut *tx)
    .await;
    match r {
        Err(sqlx::Error::Database(d)) if d.is_unique_violation() => {
            return Err(AppError::Conflict(
                "you've already reported this file".into(),
            ));
        }
        r => r?,
    };
    for b in &req.boxes {
        sqlx::query("INSERT INTO report_boxes (report_id, admin_id, sealed) VALUES (?, ?, ?)")
            .bind(&req.id)
            .bind(&b.admin_id)
            .bind(&b.sealed.0)
            .execute(&mut *tx)
            .await?;
    }
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}

/// Of `ids`, the versions an open report is holding.
pub async fn held(db: &SqlitePool, ids: &[String]) -> Result<HashSet<String>> {
    let mut out = HashSet::new();
    for id in ids {
        let n: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM reports WHERE version_id = ? AND status = 'open'",
        )
        .bind(id)
        .fetch_one(db)
        .await?;
        if n > 0 {
            out.insert(id.clone());
        }
    }
    Ok(out)
}

/// Delete versions' blobs, except those an open report is holding: they go
/// when the report is closed (`release`).
pub async fn delete_versions(state: &AppState, ids: &[String]) {
    let keep = match held(&state.db, ids).await {
        Ok(k) => k,
        // Keep everything rather than lose what a report needs; `check`
        // lists what's left over.
        Err(e) => {
            tracing::warn!(error = %e, "couldn't check reports before deleting blobs");
            return;
        }
    };
    let gone: Vec<String> = ids.iter().filter(|i| !keep.contains(*i)).cloned().collect();
    state.blobs.delete_versions(&gone).await;
}
