//! Authorisation: who may touch which node.
//!
//! A user has `Owner` access to every node in their own tree, and the best
//! permission of any share on the node or one of its ancestors otherwise.
//! Expired shares count for nothing (the janitor deletes them later).
//!
//! Nodes in the trash (or below a trashed folder) are invisible to all of
//! this: only the trash routes, which check ownership themselves, can reach
//! them. The same goes for dropped files not yet taken in by their owner,
//! which only `routes/drops.rs` reaches.

use sqlx::SqlitePool;

use crate::error::{AppError, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Access {
    Read,
    Write,
    Owner,
}

/// `anc(id, parent_id, depth)`: the node bound to the first `?` and all its
/// ancestors, depth 0 being the node itself.
macro_rules! ancestors_cte {
    () => {
        "WITH RECURSIVE anc(id, parent_id, depth) AS ( \
            SELECT id, parent_id, 0 FROM nodes WHERE id = ? \
            UNION ALL \
            SELECT n.id, n.parent_id, anc.depth + 1 FROM nodes n JOIN anc ON n.id = anc.parent_id \
         ) "
    };
}

/// `sub(id)`: the node bound to the first `?` and all its descendants.
#[macro_export]
macro_rules! subtree_cte {
    () => {
        "WITH RECURSIVE sub(id) AS ( \
            SELECT id FROM nodes WHERE id = ? \
            UNION ALL \
            SELECT n.id FROM nodes n JOIN sub ON n.parent_id = sub.id \
         ) "
    };
}

pub async fn access(db: &SqlitePool, user_id: &str, node_id: &str) -> Result<Option<Access>> {
    let owner: Option<String> = sqlx::query_scalar("SELECT owner_id FROM nodes WHERE id = ?")
        .bind(node_id)
        .fetch_optional(db)
        .await?;
    let Some(owner) = owner else { return Ok(None) };
    if is_trashed(db, node_id).await? {
        return Ok(None);
    }
    if owner == user_id {
        return Ok(Some(Access::Owner));
    }
    let perms: Vec<String> = sqlx::query_scalar(concat!(
        ancestors_cte!(),
        "SELECT s.permission FROM shares s JOIN anc ON s.node_id = anc.id WHERE s.recipient_id = ? \
         AND (s.expires_at IS NULL OR s.expires_at > ?)"
    ))
    .bind(node_id)
    .bind(user_id)
    .bind(crate::util::now())
    .fetch_all(db)
    .await?;
    Ok(perms
        .iter()
        .map(|p| {
            if p == "write" {
                Access::Write
            } else {
                Access::Read
            }
        })
        .max())
}

/// True if the node or any of its ancestors is in the trash, or it is a
/// dropped file its owner hasn't taken in yet (see `routes/drops.rs`).
pub async fn is_trashed(db: &SqlitePool, node_id: &str) -> Result<bool> {
    Ok(sqlx::query_scalar(concat!(
        ancestors_cte!(),
        "SELECT EXISTS(SELECT 1 FROM anc JOIN nodes n ON n.id = anc.id WHERE n.trashed_at IS NOT NULL OR n.dropped = 1)"
    ))
    .bind(node_id)
    .fetch_one(db)
    .await?)
}

/// Require at least `min` access. Nodes the user can't see at all are
/// reported as missing so their existence isn't leaked.
pub async fn require(db: &SqlitePool, user_id: &str, node_id: &str, min: Access) -> Result<Access> {
    match access(db, user_id, node_id).await? {
        None => Err(AppError::NotFound),
        Some(a) if a < min => Err(AppError::Forbidden),
        Some(a) => Ok(a),
    }
}

/// Ids of the node and its ancestors, ordered from the node up to the root.
pub async fn ancestors(db: &SqlitePool, node_id: &str) -> Result<Vec<String>> {
    Ok(sqlx::query_scalar(concat!(
        ancestors_cte!(),
        "SELECT id FROM anc ORDER BY depth"
    ))
    .bind(node_id)
    .fetch_all(db)
    .await?)
}

/// Shares for `user_id` on the node or its ancestors, nearest first:
/// `(share_id, node_id, wrapped_key, permission)`.
pub async fn shares_on_path(
    db: &SqlitePool,
    user_id: &str,
    node_id: &str,
) -> Result<Vec<(String, String, Vec<u8>, String)>> {
    Ok(sqlx::query_as(concat!(
        ancestors_cte!(),
        "SELECT s.id, s.node_id, s.wrapped_key, s.permission FROM shares s JOIN anc ON s.node_id = anc.id \
         WHERE s.recipient_id = ? AND (s.expires_at IS NULL OR s.expires_at > ?) ORDER BY anc.depth"
    ))
    .bind(node_id)
    .bind(user_id)
    .bind(crate::util::now())
    .fetch_all(db)
    .await?)
}

/// True if `node_id` is `root_id` or lies beneath it.
pub async fn is_within(db: &SqlitePool, node_id: &str, root_id: &str) -> Result<bool> {
    Ok(ancestors(db, node_id).await?.iter().any(|a| a == root_id))
}
