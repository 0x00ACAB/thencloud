//! Storage alerts for admins: how full the disks and the bucket are, and
//! which accounts are close to their quota. Plain counts, like the stats.

use std::path::Path;

use thencloud_crypto::api::{AdminAlerts, QuotaAlert, StorageSpace};

use crate::AppState;
use crate::error::Result;

/// An account counts as near its quota from this share of it on.
pub const NEAR_QUOTA_PERCENT: i64 = 90;
/// At most this many accounts are listed.
const NEAR_QUOTA_LISTED: i64 = 50;
/// A disk or bucket is low with less than this share free...
const LOW_PERCENT: u64 = 10;
/// ...or, for a disk, less than this many bytes.
const LOW_DISK_BYTES: u64 = 2 * 1024 * 1024 * 1024;

/// Size and free space of the filesystem holding `path`, and an id for
/// that filesystem (so two paths on one disk are reported once).
#[cfg(unix)]
pub fn disk_space(path: &Path) -> Option<(u64, u64, u64)> {
    use std::os::unix::ffi::OsStrExt;
    use std::os::unix::fs::MetadataExt;
    let c = std::ffi::CString::new(path.as_os_str().as_bytes()).ok()?;
    let mut s: libc::statvfs = unsafe { std::mem::zeroed() };
    if unsafe { libc::statvfs(c.as_ptr(), &mut s) } != 0 {
        return None;
    }
    let unit = s.f_frsize as u64;
    let dev = std::fs::metadata(path).ok()?.dev();
    // f_bavail: what an unprivileged process can still write.
    Some((s.f_blocks as u64 * unit, s.f_bavail as u64 * unit, dev))
}

#[cfg(not(unix))]
pub fn disk_space(_path: &Path) -> Option<(u64, u64, u64)> {
    None
}

fn disk_low(total: u64, free: u64) -> bool {
    free.saturating_mul(100) < total.saturating_mul(LOW_PERCENT) || free < LOW_DISK_BYTES
}

fn bucket_low(capacity: u64, used: u64) -> bool {
    capacity.saturating_sub(used).saturating_mul(100) < capacity.saturating_mul(LOW_PERCENT)
}

pub async fn alerts(state: &AppState) -> Result<AdminAlerts> {
    let mut spaces = Vec::new();
    let mut seen = Vec::new();
    let data_dir = state.config.data_dir.clone();
    let blob_root = state.blobs.local_root().map(Path::to_path_buf);
    let disks = tokio::task::spawn_blocking(move || {
        let mut out = vec![("data", disk_space(&data_dir))];
        if let Some(root) = blob_root {
            // The blob directory is made on first write; until then it's on
            // the data directory's disk anyway.
            out.push(("blobs", disk_space(&root)));
        }
        out
    })
    .await
    .unwrap_or_default();
    for (place, found) in disks {
        let Some((total, free, dev)) = found else {
            continue;
        };
        if seen.contains(&dev) {
            continue;
        }
        seen.push(dev);
        spaces.push(StorageSpace {
            place: place.into(),
            total_bytes: total as i64,
            free_bytes: free as i64,
            low: disk_low(total, free),
        });
    }
    if let (Some(capacity), Some(_)) = (state.config.s3_capacity, state.blobs.s3_target()) {
        // Ciphertext the accounts hold here (not what's on linked storage),
        // which is what's in the bucket, give or take database snapshots.
        let used: i64 = sqlx::query_scalar("SELECT COALESCE(SUM(used_bytes), 0) FROM users")
            .fetch_one(&state.db)
            .await?;
        let used = used.max(0) as u64;
        spaces.push(StorageSpace {
            place: "bucket".into(),
            total_bytes: capacity as i64,
            free_bytes: capacity.saturating_sub(used) as i64,
            low: bucket_low(capacity, used),
        });
    }
    let near_quota = sqlx::query_as::<_, (String, i64, i64)>(
        "SELECT username, used_bytes, quota_bytes FROM users
         WHERE disabled_at IS NULL AND quota_bytes > 0 AND used_bytes * 100 >= quota_bytes * ?
         ORDER BY CAST(used_bytes AS REAL) / quota_bytes DESC, username LIMIT ?",
    )
    .bind(NEAR_QUOTA_PERCENT)
    .bind(NEAR_QUOTA_LISTED)
    .fetch_all(&state.db)
    .await?
    .into_iter()
    .map(|(username, used_bytes, quota_bytes)| QuotaAlert {
        username,
        used_bytes,
        quota_bytes,
    })
    .collect();
    Ok(AdminAlerts { spaces, near_quota })
}

#[cfg(test)]
mod tests {
    use super::*;

    const GIB: u64 = 1024 * 1024 * 1024;

    #[test]
    fn disks_are_low_under_a_tenth_or_two_gib_free() {
        assert!(!disk_low(100 * GIB, 50 * GIB));
        assert!(!disk_low(100 * GIB, 10 * GIB));
        assert!(disk_low(100 * GIB, 9 * GIB));
        assert!(disk_low(10 * GIB, GIB + GIB / 2), "under 2 GiB free");
        assert!(disk_low(0, 0));
    }

    #[test]
    fn buckets_are_low_under_a_tenth_free() {
        assert!(!bucket_low(10 * GIB, 5 * GIB));
        assert!(!bucket_low(10 * GIB, 9 * GIB));
        assert!(bucket_low(10 * GIB, 9 * GIB + 1));
        assert!(bucket_low(10 * GIB, 20 * GIB), "over capacity");
    }

    #[cfg(unix)]
    #[test]
    fn measures_a_real_disk() {
        let (total, free, _) = disk_space(Path::new("/")).unwrap();
        assert!(total > 0 && free <= total);
    }
}
