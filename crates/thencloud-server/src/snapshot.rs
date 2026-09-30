//! Database snapshots in the S3 bucket, so the server can be rebuilt from
//! the bucket alone when its disk is lost.
//!
//! The blobs are only half of it: the database holds the accounts, the
//! wrapped keys, every node and share, and the server's own secret. With
//! S3 configured, a consistent snapshot of it (SQLite `VACUUM INTO`) goes
//! to `<prefix>db/thencloud-<time>.db` every `--s3-snapshot-hours`, and all
//! but the newest `--s3-snapshots-kept` are deleted. Like the blobs, it
//! holds nothing the server couldn't already see: no key, password or
//! plaintext.
//!
//! `thencloud-server restore-snapshot` puts the newest one back as the
//! data directory's database (see `restore`).

use std::path::Path;
use std::time::Duration;

use sqlx::SqlitePool;

use crate::AppState;
use crate::error::{AppError, Result};
use crate::s3::S3Target;
use crate::util::now;

const DIR: &str = "db/";
const NAME: &str = "thencloud-";
const EXT: &str = ".db";

/// "20260930T120000Z" for seconds since the epoch: sorts by time as text.
fn stamp(t: i64) -> String {
    let days = t.div_euclid(86_400);
    let secs = t.rem_euclid(86_400);
    // Civil date from days (Howard Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!(
        "{y:04}{m:02}{d:02}T{:02}{:02}{:02}Z",
        secs / 3600,
        secs / 60 % 60,
        secs % 60
    )
}

/// Seconds since the epoch from a stamp, or None.
fn parse_stamp(s: &str) -> Option<i64> {
    let b = s.as_bytes();
    if b.len() != 16 || b[8] != b'T' || b[15] != b'Z' {
        return None;
    }
    let n = |r: std::ops::Range<usize>| s.get(r)?.parse::<i64>().ok();
    let (y, m, d) = (n(0..4)?, n(4..6)?, n(6..8)?);
    let (hh, mm, ss) = (n(9..11)?, n(11..13)?, n(13..15)?);
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * ((m + 9) % 12) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    Some((era * 146_097 + doe - 719_468) * 86_400 + hh * 3600 + mm * 60 + ss)
}

/// The snapshots in the bucket, oldest first: `(key, time)`.
pub async fn list(t: &S3Target) -> Result<Vec<(String, i64)>> {
    let dir = t.key(DIR);
    let mut out: Vec<(String, i64)> = t
        .list(&dir)
        .await?
        .into_iter()
        .filter_map(|k| {
            let name = k
                .strip_prefix(&dir)?
                .strip_prefix(NAME)?
                .strip_suffix(EXT)?;
            parse_stamp(name).map(|at| (k.clone(), at))
        })
        .collect();
    out.sort_by_key(|(_, at)| *at);
    Ok(out)
}

/// Take one snapshot now and prune old ones; returns its key.
pub async fn take(db: &SqlitePool, t: &S3Target, data_dir: &Path, keep: usize) -> Result<String> {
    // VACUUM INTO needs a file; next to the database, so it's on the same disk.
    let tmp = data_dir.join(format!(".snapshot-{}.db", uuid::Uuid::new_v4().simple()));
    let path = tmp
        .to_str()
        .ok_or_else(|| AppError::bad("the data directory must be valid UTF-8"))?;
    let made = async {
        sqlx::query("VACUUM INTO ?").bind(path).execute(db).await?;
        let bytes = tokio::fs::read(&tmp).await?;
        let key = t.key(&format!("{DIR}{NAME}{}{EXT}", stamp(now())));
        t.put(&key, &bytes).await?;
        Ok::<String, AppError>(key)
    }
    .await;
    let _ = tokio::fs::remove_file(&tmp).await;
    let key = made?;
    let all = list(t).await?;
    if all.len() > keep.max(1) {
        let old: Vec<String> = all[..all.len() - keep.max(1)]
            .iter()
            .map(|(k, _)| k.clone())
            .collect();
        t.delete_many(&old).await?;
    }
    Ok(key)
}

/// Keep snapshots coming while the server runs: one at start if the newest
/// is older than the interval (or there is none), then every interval.
pub fn spawn(state: AppState) {
    let hours = state.config.s3_snapshot_hours;
    let Some(t) = state.blobs.s3_target().cloned() else {
        return;
    };
    if hours == 0 {
        return;
    }
    let every = hours as i64 * 3600;
    tokio::spawn(async move {
        loop {
            let newest = match list(&t).await {
                Ok(all) => all.last().map(|(_, at)| *at),
                Err(e) => {
                    tracing::warn!(error = %e, "couldn't list database snapshots");
                    None
                }
            };
            let due = newest.map_or(0, |at| at + every);
            let wait = (due - now()).max(0);
            if wait > 0 {
                tokio::select! {
                    // Look again at least hourly, in case the clock or the
                    // bucket changed underneath.
                    () = tokio::time::sleep(Duration::from_secs(wait.min(3600) as u64)) => continue,
                    () = state.shutdown.cancelled() => return,
                }
            }
            match take(
                &state.db,
                &t,
                &state.config.data_dir,
                state.config.s3_snapshots_kept,
            )
            .await
            {
                Ok(key) => tracing::info!(key, "database snapshot saved to the bucket"),
                Err(e) => {
                    tracing::warn!(error = %e, "database snapshot failed; trying again in an hour");
                    tokio::select! {
                        () = tokio::time::sleep(Duration::from_secs(3600)) => {}
                        () = state.shutdown.cancelled() => return,
                    }
                }
            }
        }
    });
}

/// Put the newest snapshot back as `data_dir/thencloud.db`, which must not
/// exist yet. Returns its key and time.
pub async fn restore(t: &S3Target, data_dir: &Path) -> Result<(String, i64)> {
    let target = data_dir.join("thencloud.db");
    if target.exists() {
        return Err(AppError::bad(format!(
            "{} already exists; restore into an empty data directory",
            target.display()
        )));
    }
    let (key, at) = list(t)
        .await?
        .pop()
        .ok_or_else(|| AppError::bad(format!("no database snapshots under {}", t.key(DIR))))?;
    let bytes = t.get(&key).await?;
    tokio::fs::create_dir_all(data_dir).await?;
    // Written whole, then renamed, so a cut-off download leaves nothing.
    let part = data_dir.join(".thencloud.db.part");
    tokio::fs::write(&part, &bytes).await?;
    tokio::fs::rename(&part, &target).await?;
    Ok((key, at))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stamps_round_trip_and_sort() {
        for t in [0, 951_782_400, 1_789_615_987, 4_102_444_799] {
            assert_eq!(parse_stamp(&stamp(t)), Some(t), "{t}");
        }
        assert_eq!(stamp(1_789_615_987), "20260917T033307Z");
        assert!(stamp(1_000) < stamp(2_000));
        for bad in [
            "",
            "20260917T033307",
            "2026-09-17T033307Z",
            "20260917X033307Z",
        ] {
            assert_eq!(parse_stamp(bad), None, "{bad}");
        }
    }
}
