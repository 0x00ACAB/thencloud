//! Maintenance from the command line: `backup` takes a consistent
//! snapshot of the database and the blobs it refers to, and `check` makes
//! sure every blob the database expects is there with the right size.
//!
//! Both only ever see ciphertext; a backup is as opaque as the server.

use std::collections::HashSet;
use std::io;
use std::path::{Path, PathBuf};

use sqlx::SqlitePool;
use sqlx::sqlite::SqliteConnectOptions;

use crate::error::{AppError, Result};

/// Where one chunk of one version lives under a blob root.
fn chunk_path(root: &Path, version_id: &str, idx: i64) -> PathBuf {
    root.join(&version_id[..2])
        .join(version_id)
        .join(idx.to_string())
}

/// Every chunk the database refers to: `(version id, index, size if known)`.
/// Finished versions know only their total size; unfinished uploads know
/// each chunk's.
async fn expected_chunks(db: &SqlitePool) -> Result<Vec<(String, i64, Option<i64>)>> {
    let mut out = Vec::new();
    let versions: Vec<(String, i64)> =
        sqlx::query_as("SELECT id, chunk_count FROM file_versions ORDER BY id")
            .fetch_all(db)
            .await?;
    for (id, count) in versions {
        out.extend((0..count).map(|i| (id.clone(), i, None)));
    }
    let partial: Vec<(String, i64, i64)> = sqlx::query_as(
        "SELECT u.version_id, c.idx, c.size FROM upload_chunks c JOIN uploads u ON u.id = c.upload_id \
         ORDER BY u.version_id, c.idx",
    )
    .fetch_all(db)
    .await?;
    out.extend(partial.into_iter().map(|(v, i, s)| (v, i, Some(s))));
    Ok(out)
}

#[derive(Debug, Default)]
pub struct BackupReport {
    pub chunks: u64,
    pub bytes: u64,
    /// Chunks deleted by the running server while the backup was made.
    pub missing: Vec<(String, i64)>,
}

/// Snapshot the database in `data_dir` into `dest/thencloud.db` and copy
/// the blobs it refers to into `dest/blobs`. `dest` is then a data
/// directory of its own: restoring is pointing `--data-dir` at a copy.
///
/// Safe while the server runs. The database snapshot is consistent (SQLite
/// `VACUUM INTO`); blobs never change once written, so only one deleted
/// between the snapshot and its copy can be missing, and those are listed.
/// Blobs are hard-linked when `dest` is on the same filesystem, which is
/// instant and takes no extra space.
pub async fn backup(db: &SqlitePool, data_dir: &Path, dest: &Path) -> Result<BackupReport> {
    if dest.exists() && std::fs::read_dir(dest)?.next().is_some() {
        return Err(AppError::bad(format!(
            "{} exists and isn't empty",
            dest.display()
        )));
    }
    tokio::fs::create_dir_all(dest).await?;
    let snapshot = dest.join("thencloud.db");
    let target = snapshot
        .to_str()
        .ok_or_else(|| AppError::bad("the backup path must be valid UTF-8"))?;
    sqlx::query("VACUUM INTO ?")
        .bind(target)
        .execute(db)
        .await?;

    let snap = SqlitePool::connect_with(
        SqliteConnectOptions::new()
            .filename(&snapshot)
            .read_only(true),
    )
    .await?;
    let chunks = expected_chunks(&snap).await;
    snap.close().await;

    let from = data_dir.join("blobs");
    let to = dest.join("blobs");
    let mut report = BackupReport::default();
    for (version, idx, _) in chunks? {
        let src = chunk_path(&from, &version, idx);
        let dst = chunk_path(&to, &version, idx);
        tokio::fs::create_dir_all(dst.parent().expect("chunk has a parent")).await?;
        let copied = match tokio::fs::hard_link(&src, &dst).await {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Err(e),
            // Another filesystem (or no hard links there): copy instead.
            Err(_) => tokio::fs::copy(&src, &dst).await.map(|_| ()),
        };
        match copied {
            Ok(()) => {
                report.chunks += 1;
                report.bytes += tokio::fs::metadata(&dst).await?.len();
            }
            Err(e) if e.kind() == io::ErrorKind::NotFound => report.missing.push((version, idx)),
            Err(e) => return Err(e.into()),
        }
    }
    Ok(report)
}

#[derive(Debug, Default)]
pub struct CheckReport {
    pub versions: u64,
    pub chunks: u64,
    /// `(version id, index)` of chunks that aren't there.
    pub missing: Vec<(String, i64)>,
    /// Versions (or upload chunks) whose stored size isn't what the
    /// database says: `(version id, expected, found)`.
    pub wrong_size: Vec<(String, u64, u64)>,
    /// Version directories no version or upload refers to.
    pub orphans: Vec<String>,
}

impl CheckReport {
    pub fn is_ok(&self) -> bool {
        self.missing.is_empty() && self.wrong_size.is_empty()
    }
}

/// Check every chunk the database expects against the blob store.
pub async fn check(db: &SqlitePool, data_dir: &Path) -> Result<CheckReport> {
    let root = data_dir.join("blobs");
    let mut report = CheckReport::default();
    let totals: Vec<(String, i64)> = sqlx::query_as("SELECT id, size FROM file_versions")
        .fetch_all(db)
        .await?;
    let totals: std::collections::HashMap<String, i64> = totals.into_iter().collect();
    report.versions = totals.len() as u64;

    let mut known: HashSet<String> = sqlx::query_scalar("SELECT version_id FROM uploads")
        .fetch_all(db)
        .await?
        .into_iter()
        .collect();
    let mut sums: std::collections::HashMap<String, u64> = Default::default();
    for (version, idx, size) in expected_chunks(db).await? {
        known.insert(version.clone());
        match tokio::fs::metadata(chunk_path(&root, &version, idx)).await {
            Ok(m) => {
                report.chunks += 1;
                match size {
                    Some(s) if s as u64 != m.len() => {
                        report
                            .wrong_size
                            .push((format!("{version}/{idx}"), s as u64, m.len()))
                    }
                    Some(_) => {}
                    None => *sums.entry(version).or_default() += m.len(),
                }
            }
            Err(e) if e.kind() == io::ErrorKind::NotFound => report.missing.push((version, idx)),
            Err(e) => return Err(e.into()),
        }
    }
    let incomplete: HashSet<&String> = report.missing.iter().map(|(v, _)| v).collect();
    for (version, total) in &totals {
        let found = sums.get(version).copied().unwrap_or(0);
        if !incomplete.contains(version) && found != *total as u64 {
            report
                .wrong_size
                .push((version.clone(), *total as u64, found));
        }
    }

    // Directories nothing refers to, left behind by a crash mid-delete.
    if let Ok(mut prefixes) = tokio::fs::read_dir(&root).await {
        while let Some(p) = prefixes.next_entry().await? {
            if !p.file_type().await?.is_dir() {
                continue;
            }
            let mut dirs = tokio::fs::read_dir(p.path()).await?;
            while let Some(d) = dirs.next_entry().await? {
                let name = d.file_name().to_string_lossy().into_owned();
                if !known.contains(&name) {
                    report.orphans.push(name);
                }
            }
        }
    }
    report.missing.sort();
    report.orphans.sort();
    Ok(report)
}
