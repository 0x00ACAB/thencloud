//! Encrypted chunk storage: the local filesystem, an S3-compatible bucket
//! (see s3.rs), or both at once (a mirror). All keep the same layout, so a
//! bucket can be copied to a directory (or back) key for key.
//!
//! Layout: `<first two chars of version id>/<version id>/<chunk index>`
//! under the local root, or under the bucket prefix for S3.
//! Version ids are validated UUIDs, so they are safe path components.
//!
//! A mirror (`--s3-mirror`) writes every chunk to both and deletes from
//! both. Reads come from the local disk, which costs nothing, and fall back
//! to the bucket, putting the chunk back on disk as they go: after a disk is
//! replaced, files refill it as they're read (and `check` lists what's
//! missing where).

use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::s3::S3Target;

#[derive(Debug, Clone)]
pub enum BlobStore {
    Local { root: PathBuf },
    S3(Arc<S3Target>),
    Mirror { root: PathBuf, s3: Arc<S3Target> },
}

/// The `<xx>/<version id>/<chunk index>` part of a chunk's path or key.
fn chunk_rel(version_id: &str, idx: u32) -> String {
    format!("{}/{version_id}/{idx}", &version_id[..2])
}

fn version_dir(root: &Path, version_id: &str) -> PathBuf {
    root.join(&version_id[..2]).join(version_id)
}

// The two kinds of storage on their own; BlobStore combines them.

/// Atomic: a temp file, then a rename.
async fn local_put(root: &Path, version_id: &str, idx: u32, data: &[u8]) -> io::Result<()> {
    let dir = version_dir(root, version_id);
    tokio::fs::create_dir_all(&dir).await?;
    let tmp = dir.join(format!("{idx}.tmp-{}", uuid::Uuid::new_v4().simple()));
    tokio::fs::write(&tmp, data).await?;
    if let Err(e) = tokio::fs::rename(&tmp, dir.join(idx.to_string())).await {
        let _ = tokio::fs::remove_file(&tmp).await;
        return Err(e);
    }
    Ok(())
}

async fn local_get(root: &Path, version_id: &str, idx: u32) -> io::Result<Vec<u8>> {
    tokio::fs::read(version_dir(root, version_id).join(idx.to_string())).await
}

async fn local_size(root: &Path, version_id: &str, idx: u32) -> io::Result<u64> {
    Ok(
        tokio::fs::metadata(version_dir(root, version_id).join(idx.to_string()))
            .await?
            .len(),
    )
}

async fn local_delete(root: &Path, version_id: &str) {
    match tokio::fs::remove_dir_all(version_dir(root, version_id)).await {
        Ok(()) => {}
        Err(e) if e.kind() == io::ErrorKind::NotFound => {}
        Err(e) => tracing::warn!(version_id, error = %e, "failed to delete blob directory"),
    }
}

async fn local_versions(root: &Path, out: &mut Vec<String>) -> io::Result<()> {
    let mut prefixes = match tokio::fs::read_dir(root).await {
        Ok(p) => p,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(e),
    };
    while let Some(p) = prefixes.next_entry().await? {
        if !p.file_type().await?.is_dir() {
            continue;
        }
        let mut dirs = tokio::fs::read_dir(p.path()).await?;
        while let Some(d) = dirs.next_entry().await? {
            out.push(d.file_name().to_string_lossy().into_owned());
        }
    }
    Ok(())
}

async fn s3_delete(t: &S3Target, version_id: &str) {
    let prefix = t.key(&format!("{}/{version_id}/", &version_id[..2]));
    let gone = async {
        let keys = t.list(&prefix).await?;
        if !keys.is_empty() {
            t.delete_many(&keys).await?;
        }
        Ok::<(), io::Error>(())
    }
    .await;
    if let Err(e) = gone {
        tracing::warn!(version_id, error = %e, "failed to delete blobs from S3");
    }
}

async fn s3_versions(t: &S3Target, out: &mut Vec<String>) -> io::Result<()> {
    for xx in t.list_dirs(&t.prefix).await? {
        for v in t.list_dirs(&xx).await? {
            if let Some(id) = v
                .strip_suffix('/')
                .and_then(|v| v.rsplit_once('/').map(|(_, id)| id.to_string()))
            {
                out.push(id);
            }
        }
    }
    Ok(())
}

impl BlobStore {
    pub fn local(root: PathBuf) -> Self {
        BlobStore::Local { root }
    }

    pub fn s3(target: S3Target) -> Self {
        BlobStore::S3(Arc::new(target))
    }

    pub fn mirror(root: PathBuf, target: S3Target) -> Self {
        BlobStore::Mirror {
            root,
            s3: Arc::new(target),
        }
    }

    /// The copies a mirror keeps, each as a store of its own (for `check`,
    /// which reports on each); one store otherwise.
    pub fn copies(&self) -> Vec<BlobStore> {
        match self {
            BlobStore::Mirror { root, s3 } => vec![
                BlobStore::Local { root: root.clone() },
                BlobStore::S3(s3.clone()),
            ],
            other => vec![other.clone()],
        }
    }

    /// The bucket, when blobs go to one (on their own or mirrored).
    pub fn s3_target(&self) -> Option<&Arc<S3Target>> {
        match self {
            BlobStore::S3(t) | BlobStore::Mirror { s3: t, .. } => Some(t),
            BlobStore::Local { .. } => None,
        }
    }

    /// Where this store is, for messages.
    pub fn describe(&self) -> String {
        match self {
            BlobStore::Local { root } => root.display().to_string(),
            BlobStore::S3(t) => format!("s3://{}/{}", t.bucket, t.prefix),
            BlobStore::Mirror { root, s3 } => {
                format!("{} and s3://{}/{}", root.display(), s3.bucket, s3.prefix)
            }
        }
    }

    /// Write one chunk. Atomic on both stores: the local one writes a temp
    /// file and renames it; an S3 PUT is atomic by itself. A mirror writes
    /// both and fails if either fails, so a stored chunk is always in both.
    pub async fn put_chunk(&self, version_id: &str, idx: u32, data: &[u8]) -> io::Result<()> {
        match self {
            BlobStore::Local { root } => local_put(root, version_id, idx, data).await,
            BlobStore::S3(t) => t.put(&t.key(&chunk_rel(version_id, idx)), data).await,
            BlobStore::Mirror { root, s3 } => {
                let key = s3.key(&chunk_rel(version_id, idx));
                let (a, b) =
                    tokio::join!(local_put(root, version_id, idx, data), s3.put(&key, data));
                a.and(b)
            }
        }
    }

    /// A chunk's bytes; `NotFound` when it isn't there.
    pub async fn get_chunk(&self, version_id: &str, idx: u32) -> io::Result<Vec<u8>> {
        match self {
            BlobStore::Local { root } => local_get(root, version_id, idx).await,
            BlobStore::S3(t) => t.get(&t.key(&chunk_rel(version_id, idx))).await,
            BlobStore::Mirror { root, s3 } => match local_get(root, version_id, idx).await {
                Ok(data) => Ok(data),
                Err(local) => {
                    let data = s3.get(&s3.key(&chunk_rel(version_id, idx))).await?;
                    // Put it back on the disk for next time.
                    if local.kind() == io::ErrorKind::NotFound
                        && let Err(e) = local_put(root, version_id, idx, &data).await
                    {
                        tracing::warn!(version_id, error = %e, "couldn't restore a chunk to disk");
                    }
                    Ok(data)
                }
            },
        }
    }

    /// A chunk's size without reading it; `NotFound` when it isn't there.
    pub async fn chunk_size(&self, version_id: &str, idx: u32) -> io::Result<u64> {
        match self {
            BlobStore::Local { root } => local_size(root, version_id, idx).await,
            BlobStore::S3(t) => t.size(&t.key(&chunk_rel(version_id, idx))).await,
            BlobStore::Mirror { root, s3 } => match local_size(root, version_id, idx).await {
                Ok(n) => Ok(n),
                Err(_) => s3.size(&s3.key(&chunk_rel(version_id, idx))).await,
            },
        }
    }

    pub async fn delete_version(&self, version_id: &str) {
        match self {
            BlobStore::Local { root } => local_delete(root, version_id).await,
            BlobStore::S3(t) => s3_delete(t, version_id).await,
            BlobStore::Mirror { root, s3 } => {
                tokio::join!(local_delete(root, version_id), s3_delete(s3, version_id));
            }
        }
    }

    pub async fn delete_versions(&self, version_ids: &[String]) {
        for v in version_ids {
            self.delete_version(v).await;
        }
    }

    /// Every version id the store holds (in either copy of a mirror),
    /// sorted. Used by `check` to find orphans; a missing local root (or an
    /// unreachable bucket prefix) simply means there is nothing.
    pub async fn list_versions(&self) -> io::Result<Vec<String>> {
        let mut out = Vec::new();
        match self {
            BlobStore::Local { root } => local_versions(root, &mut out).await?,
            BlobStore::S3(t) => s3_versions(t, &mut out).await?,
            BlobStore::Mirror { root, s3 } => {
                local_versions(root, &mut out).await?;
                s3_versions(s3, &mut out).await?;
            }
        }
        out.sort();
        out.dedup();
        Ok(out)
    }
}
