//! Maintenance from the command line: `backup` takes a consistent
//! snapshot of the database and the blobs it refers to, and `check` makes
//! sure every blob the database expects is there with the right size.
//!
//! Both only ever see ciphertext; a backup is as opaque as the server.
//! Blobs are read through the blob store (local or S3) and written to a
//! destination that can be either: a directory, or another S3 prefix.

use std::collections::HashSet;
use std::io;
use std::path::{Path, PathBuf};

use futures_util::StreamExt;
use sqlx::SqlitePool;
use sqlx::sqlite::SqliteConnectOptions;

use crate::Config;
use crate::blob::BlobStore;
use crate::error::{AppError, Result};
use crate::s3::S3Target;

/// How many chunk copies (or size checks) run at once. S3 latency makes a
/// sequential crawl of a big store unbearably slow; the local store doesn't
/// mind either.
const PARALLEL: usize = 16;

/// One copied chunk: its size in bytes, `Err(true)` when the source had
/// already lost it, `Err(false)` when the copy itself failed.
type Copied = std::result::Result<u64, bool>;

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

/// Where a backup goes: a new directory, or an S3 bucket prefix.
#[derive(Debug, Clone)]
pub enum BackupDest {
    Dir(PathBuf),
    S3(S3Target),
}

impl BackupDest {
    /// Parse a destination: a directory path, or `s3://BUCKET/PREFIX`
    /// (which uses the endpoint, region and credentials from the config).
    pub fn parse(spec: &str, cfg: &Config) -> Result<Self> {
        let Some(rest) = spec.strip_prefix("s3://") else {
            return Ok(BackupDest::Dir(PathBuf::from(spec)));
        };
        let (bucket, prefix) = match rest.split_once('/') {
            Some((b, p)) => (b, p),
            None => (rest, ""),
        };
        if bucket.is_empty() {
            return Err(AppError::bad("s3:// destination needs a bucket"));
        }
        let (Some(endpoint), Some(access), Some(secret)) = (
            cfg.s3_endpoint.as_deref(),
            cfg.s3_access_key.as_deref(),
            cfg.s3_secret_key.as_deref(),
        ) else {
            return Err(AppError::bad(
                "an s3:// destination needs --s3-endpoint, --s3-access-key and --s3-secret-key",
            ));
        };
        Ok(BackupDest::S3(crate::s3::target(
            endpoint,
            &cfg.s3_region,
            bucket.to_string(),
            access,
            secret,
            prefix,
        )))
    }

    fn describe(&self) -> String {
        match self {
            BackupDest::Dir(p) => p.display().to_string(),
            BackupDest::S3(t) => format!("s3://{}/{}", t.bucket, t.prefix),
        }
    }

    /// Refuse to write over something that is already there.
    async fn refuse_unless_empty(&self) -> Result<()> {
        let busy = match self {
            BackupDest::Dir(p) => p.exists() && std::fs::read_dir(p)?.next().is_some(),
            BackupDest::S3(t) => !t.is_empty(&t.prefix).await?,
        };
        if busy {
            return Err(AppError::bad(format!(
                "{} exists and isn't empty",
                self.describe()
            )));
        }
        Ok(())
    }

    /// Store the database snapshot where the backup lives.
    async fn put_db(&self, snapshot: &Path) -> Result<()> {
        match self {
            BackupDest::Dir(_) => Ok(()), // VACUUM INTO wrote it in place
            BackupDest::S3(t) => {
                let bytes = tokio::fs::read(snapshot).await?;
                t.put(&t.key("thencloud.db"), &bytes).await?;
                Ok(())
            }
        }
    }

    /// Store one chunk of one version.
    async fn put_chunk(&self, version_id: &str, idx: i64, data: &[u8]) -> io::Result<()> {
        match self {
            BackupDest::Dir(dest) => {
                let dst = chunk_path(&dest.join("blobs"), version_id, idx);
                tokio::fs::create_dir_all(dst.parent().expect("chunk has a parent")).await?;
                tokio::fs::write(&dst, data).await
            }
            BackupDest::S3(t) => {
                t.put(
                    &t.key(&format!("blobs/{}/{version_id}/{idx}", &version_id[..2])),
                    data,
                )
                .await
            }
        }
    }
}

#[derive(Debug, Default)]
pub struct BackupReport {
    pub chunks: u64,
    pub bytes: u64,
    /// Chunks deleted by the running server while the backup was made.
    pub missing: Vec<(String, i64)>,
}

/// Copy `<data dir>/link-token-key` into a directory backup, so the restored
/// server can still show owners their public links (see link_tokens.rs).
/// An S3 backup doesn't get it: keeping it apart from the database is the
/// point. Returns whether there was one to copy.
pub async fn copy_link_token_key(data_dir: &Path, dest: &BackupDest) -> Result<bool> {
    let BackupDest::Dir(dir) = dest else {
        return Ok(false);
    };
    let from = data_dir.join("link-token-key");
    if !from.exists() {
        return Ok(false);
    }
    let to = dir.join("link-token-key");
    tokio::fs::copy(&from, &to).await?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        tokio::fs::set_permissions(&to, std::fs::Permissions::from_mode(0o600)).await?;
    }
    Ok(true)
}

/// Snapshot the database into `dest` as `thencloud.db` and copy the blobs
/// it refers to under `dest/blobs`. A directory destination is then a data
/// directory of its own: restoring is pointing `--data-dir` at a copy. To
/// restore an S3 destination, put its `thencloud.db` in a data directory
/// and run with `--s3-prefix` pointing at the backup's `blobs/` prefix.
///
/// Safe while the server runs. The database snapshot is consistent (SQLite
/// `VACUUM INTO`); blobs never change once written, so only one deleted
/// between the snapshot and its copy can be missing, and those are listed.
/// Between two local directories blobs are hard-linked where possible,
/// which is instant and takes no extra space.
pub async fn backup(db: &SqlitePool, blobs: &BlobStore, dest: &BackupDest) -> Result<BackupReport> {
    dest.refuse_unless_empty().await?;

    // The snapshot is always a local file first: VACUUM INTO needs one.
    let tmp;
    let snapshot = match dest {
        BackupDest::Dir(dir) => {
            tokio::fs::create_dir_all(dir).await?;
            dir.join("thencloud.db")
        }
        BackupDest::S3(_) => {
            tmp = std::env::temp_dir().join(format!(
                "thencloud-backup-{}.db",
                uuid::Uuid::new_v4().simple()
            ));
            tmp.clone()
        }
    };
    let target = snapshot
        .to_str()
        .ok_or_else(|| AppError::bad("the backup path must be valid UTF-8"))?;
    let vacuum = sqlx::query("VACUUM INTO ?").bind(target).execute(db);
    let chunks = async {
        vacuum.await?;
        let snap = SqlitePool::connect_with(
            SqliteConnectOptions::new()
                .filename(&snapshot)
                .read_only(true),
        )
        .await?;
        let chunks = expected_chunks(&snap).await;
        snap.close().await;
        dest.put_db(&snapshot).await?;
        chunks
    }
    .await;
    if matches!(dest, BackupDest::S3(_)) {
        let _ = tokio::fs::remove_file(&snapshot).await;
    }
    let chunks = chunks?;

    let mut report = BackupReport::default();
    // Local to local: hard-link (or copy) straight from the blob root.
    if let (BlobStore::Local { root } | BlobStore::Mirror { root, .. }, BackupDest::Dir(dir)) =
        (blobs, dest)
    {
        let from = root.clone();
        let to = dir.join("blobs");
        for (version, idx, _) in chunks {
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
                Err(e) if e.kind() == io::ErrorKind::NotFound => {
                    report.missing.push((version, idx))
                }
                Err(e) => return Err(e.into()),
            }
        }
        return Ok(report);
    }

    // Anything else goes through the store: read a chunk, write a chunk.
    let dest2 = dest.clone();
    let blobs2 = blobs.clone();
    let copied: Vec<(String, i64, Copied)> = futures_util::stream::iter(chunks)
        .map(|(version, idx, _)| {
            let dest = dest2.clone();
            let blobs = blobs2.clone();
            async move {
                let r = match blobs.get_chunk(&version, idx as u32).await {
                    Ok(data) => match dest.put_chunk(&version, idx, &data).await {
                        Ok(()) => Ok(data.len() as u64),
                        Err(_) => Err(false), // a real failure, not a missing chunk
                    },
                    Err(e) if e.kind() == io::ErrorKind::NotFound => Err(true),
                    Err(_) => Err(false),
                };
                (version, idx, r)
            }
        })
        .buffer_unordered(PARALLEL)
        .collect()
        .await;
    let mut failed = None;
    for (version, idx, r) in copied {
        match r {
            Ok(len) => {
                report.chunks += 1;
                report.bytes += len;
            }
            Err(true) => report.missing.push((version, idx)),
            Err(false) if failed.is_none() => failed = Some((version, idx)),
            Err(false) => {}
        }
    }
    if let Some((version, idx)) = failed {
        // Re-run one to surface the actual error.
        let data = blobs.get_chunk(&version, idx as u32).await?;
        dest.put_chunk(&version, idx, &data).await?;
    }
    report.missing.sort();
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
    /// Version directories nothing refers to.
    pub orphans: Vec<String>,
}

impl CheckReport {
    pub fn is_ok(&self) -> bool {
        self.missing.is_empty() && self.wrong_size.is_empty()
    }
}

/// Check every chunk the database expects against the blob store.
pub async fn check(db: &SqlitePool, blobs: &BlobStore) -> Result<CheckReport> {
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
    let expected = expected_chunks(db).await?;
    let blobs2 = blobs.clone();
    let sized: Vec<(String, i64, Option<i64>, Copied)> = futures_util::stream::iter(expected)
        .map(|(version, idx, size)| {
            let blobs = blobs2.clone();
            async move {
                let r = match blobs.chunk_size(&version, idx as u32).await {
                    Ok(len) => Ok(len),
                    Err(e) if e.kind() == io::ErrorKind::NotFound => Err(true),
                    Err(_) => Err(false),
                };
                (version, idx, size, r)
            }
        })
        .buffer_unordered(PARALLEL)
        .collect()
        .await;
    let mut sums: std::collections::HashMap<String, u64> = Default::default();
    let mut failed = None;
    for (version, idx, size, r) in sized {
        known.insert(version.clone());
        match r {
            Ok(len) => {
                report.chunks += 1;
                match size {
                    Some(s) if s as u64 != len => {
                        report
                            .wrong_size
                            .push((format!("{version}/{idx}"), s as u64, len))
                    }
                    Some(_) => {}
                    None => *sums.entry(version).or_default() += len,
                }
            }
            Err(true) => report.missing.push((version, idx)),
            Err(false) if failed.is_none() => failed = Some((version, idx)),
            Err(false) => {}
        }
    }
    if let Some((version, idx)) = failed {
        // Surface the actual error.
        blobs.chunk_size(&version, idx as u32).await?;
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

    // Version directories nothing refers to, left behind by a crash mid-delete.
    for id in blobs.list_versions().await? {
        if !known.contains(&id) {
            report.orphans.push(id);
        }
    }
    report.missing.sort();
    report.orphans.sort();
    Ok(report)
}
