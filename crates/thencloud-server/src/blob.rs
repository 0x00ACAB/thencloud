//! Encrypted chunk storage: the local filesystem, or an S3-compatible
//! bucket (see s3.rs). Both keep the same layout, so a bucket can be
//! copied to a directory (or back) key for key.
//!
//! Layout: `<first two chars of version id>/<version id>/<chunk index>`
//! under the local root, or under the bucket prefix for S3.
//! Version ids are validated UUIDs, so they are safe path components.

use std::io;
use std::path::PathBuf;
use std::sync::Arc;

use crate::s3::S3Target;

#[derive(Debug, Clone)]
pub enum BlobStore {
    Local { root: PathBuf },
    S3(Arc<S3Target>),
}

/// The `<xx>/<version id>/<chunk index>` part of a chunk's path or key.
fn chunk_rel(version_id: &str, idx: u32) -> String {
    format!("{}/{version_id}/{idx}", &version_id[..2])
}

impl BlobStore {
    pub fn local(root: PathBuf) -> Self {
        BlobStore::Local { root }
    }

    pub fn s3(target: S3Target) -> Self {
        BlobStore::S3(Arc::new(target))
    }

    fn version_dir(&self, version_id: &str) -> PathBuf {
        match self {
            BlobStore::Local { root } => root.join(&version_id[..2]).join(version_id),
            BlobStore::S3(_) => unreachable!("S3 has no directories"),
        }
    }

    /// Write one chunk. Atomic on both stores: the local one writes a temp
    /// file and renames it; an S3 PUT is atomic by itself.
    pub async fn put_chunk(&self, version_id: &str, idx: u32, data: &[u8]) -> io::Result<()> {
        match self {
            BlobStore::Local { .. } => {
                let dir = self.version_dir(version_id);
                tokio::fs::create_dir_all(&dir).await?;
                let tmp = dir.join(format!("{idx}.tmp-{}", uuid::Uuid::new_v4().simple()));
                tokio::fs::write(&tmp, data).await?;
                if let Err(e) = tokio::fs::rename(&tmp, dir.join(idx.to_string())).await {
                    let _ = tokio::fs::remove_file(&tmp).await;
                    return Err(e);
                }
                Ok(())
            }
            BlobStore::S3(t) => t.put(&t.key(&chunk_rel(version_id, idx)), data).await,
        }
    }

    /// A chunk's bytes; `NotFound` when it isn't there.
    pub async fn get_chunk(&self, version_id: &str, idx: u32) -> io::Result<Vec<u8>> {
        match self {
            BlobStore::Local { .. } => {
                tokio::fs::read(self.version_dir(version_id).join(idx.to_string())).await
            }
            BlobStore::S3(t) => t.get(&t.key(&chunk_rel(version_id, idx))).await,
        }
    }

    /// A chunk's size without reading it; `NotFound` when it isn't there.
    pub async fn chunk_size(&self, version_id: &str, idx: u32) -> io::Result<u64> {
        match self {
            BlobStore::Local { .. } => Ok(tokio::fs::metadata(
                self.version_dir(version_id).join(idx.to_string()),
            )
            .await?
            .len()),
            BlobStore::S3(t) => t.size(&t.key(&chunk_rel(version_id, idx))).await,
        }
    }

    pub async fn delete_version(&self, version_id: &str) {
        match self {
            BlobStore::Local { .. } => {
                match tokio::fs::remove_dir_all(self.version_dir(version_id)).await {
                    Ok(()) => {}
                    Err(e) if e.kind() == io::ErrorKind::NotFound => {}
                    Err(e) => {
                        tracing::warn!(version_id, error = %e, "failed to delete blob directory")
                    }
                }
            }
            BlobStore::S3(t) => {
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
        }
    }

    pub async fn delete_versions(&self, version_ids: &[String]) {
        for v in version_ids {
            self.delete_version(v).await;
        }
    }

    /// Every version id the store holds, sorted. Used by `check` to find
    /// orphans; a missing local root (or an unreachable bucket prefix)
    /// simply means there is nothing.
    pub async fn list_versions(&self) -> io::Result<Vec<String>> {
        let mut out = Vec::new();
        match self {
            BlobStore::Local { root } => {
                let mut prefixes = match tokio::fs::read_dir(root).await {
                    Ok(p) => p,
                    Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(out),
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
            }
            BlobStore::S3(t) => {
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
            }
        }
        out.sort();
        Ok(out)
    }
}
