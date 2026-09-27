//! Encrypted chunk storage on the local filesystem.
//!
//! Layout: `<root>/<first two chars of version id>/<version id>/<chunk index>`.
//! Version ids are validated UUIDs, so they are safe path components.

use std::io;
use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct BlobStore {
    root: PathBuf,
}

impl BlobStore {
    pub fn new(root: PathBuf) -> Self {
        BlobStore { root }
    }

    fn version_dir(&self, version_id: &str) -> PathBuf {
        self.root.join(&version_id[..2]).join(version_id)
    }

    /// Write one chunk atomically (temp file + rename).
    pub async fn put_chunk(&self, version_id: &str, idx: u32, data: &[u8]) -> io::Result<()> {
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

    pub async fn get_chunk(&self, version_id: &str, idx: u32) -> io::Result<Vec<u8>> {
        tokio::fs::read(self.version_dir(version_id).join(idx.to_string())).await
    }

    pub async fn delete_version(&self, version_id: &str) {
        match tokio::fs::remove_dir_all(self.version_dir(version_id)).await {
            Ok(()) => {}
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(e) => tracing::warn!(version_id, error = %e, "failed to delete blob directory"),
        }
    }

    pub async fn delete_versions(&self, version_ids: &[String]) {
        for v in version_ids {
            self.delete_version(v).await;
        }
    }
}
