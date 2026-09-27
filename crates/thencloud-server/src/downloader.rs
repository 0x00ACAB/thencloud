//! The optional video downloader: runs yt-dlp for a link and streams the
//! result straight to the browser, which encrypts and uploads it like any
//! other file.
//!
//! This is the one place the server handles plaintext it didn't get as
//! ciphertext, so it's off by default (an admin enables it) and it keeps as
//! little as possible:
//! - nothing is written to disk: everything goes through pipes (`-o -`).
//!   When ffmpeg is installed, separate video and audio streams are merged
//!   and remuxed on the fly into a fragmented MP4 (which can be written as a
//!   stream); without it, only formats a site offers as one file are used.
//!   yt-dlp and ffmpeg run in a
//!   scratch directory that is deleted afterwards, with its cache and config
//!   files turned off;
//! - links are never logged;
//! - only yt-dlp's site extractors run (its "any URL" generic extractor is
//!   off), and links to private or local addresses are refused, so it
//!   can't be used to reach the server's own network;
//! - one download per user at a time, with a size cap, and yt-dlp is killed
//!   as soon as the browser goes away.

use std::collections::HashSet;
use std::future::Future;
use std::net::IpAddr;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::process::Stdio;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};
use std::time::Duration;

use axum::body::Bytes;
use futures_core::Stream;
use thencloud_crypto::api::{VideoInfo, VideoKind, VideoOption};
use tokio::process::{ChildStdout, Command};
use tokio::sync::oneshot;
use tokio_util::io::ReaderStream;

use crate::error::{AppError, Result};
use crate::util::random_token;

/// With ffmpeg: the best H.264 video up to 1080p (plays almost
/// everywhere), else any video up to 1080p, with AAC audio, else one file
/// with AAC audio. AAC because the remux below needs it.
const VIDEO_MERGED: &str = "bv*[vcodec^=avc1][height<=1080]+ba[acodec^=mp4a]/bv*[height<=1080]+ba[acodec^=mp4a]/b[vcodec!=none][acodec^=mp4a]";
/// Without ffmpeg: formats offered as a single file only.
const VIDEO_SINGLE: &str =
    "best[ext=mp4][vcodec!=none][acodec!=none]/best[vcodec!=none][acodec!=none]";
const AUDIO_FORMAT: &str = "bestaudio[ext=m4a]/bestaudio";
/// yt-dlp picks the container it writes merged video to stdout in (often
/// MPEG-TS), so it goes through ffmpeg: streams copied, not re-encoded, into
/// a fragmented MP4, which can be written to a pipe and plays everywhere.
const REMUX_ARGS: &[&str] = &[
    "-hide_banner",
    "-loglevel",
    "error",
    "-i",
    "pipe:0",
    "-map",
    "0:v:0?",
    "-map",
    "0:a:0?",
    "-c",
    "copy",
    "-bsf:a",
    "aac_adtstoasc",
    "-f",
    "mp4",
    "-movflags",
    "+frag_keyframe+empty_moov+default_base_moof",
    "pipe:1",
];

/// Arguments used for every run.
const COMMON: &[&str] = &[
    "--ignore-config",
    "--no-cache-dir",
    "--no-playlist",
    "--no-warnings",
    "--no-progress",
    "--no-part",
    "--no-mtime",
    "--use-extractors",
    "default,-generic",
];

pub struct Downloader {
    program: PathBuf,
    ffmpeg: PathBuf,
    /// yt-dlp's version, if it was found at startup.
    pub version: Option<String>,
    /// Whether ffmpeg is there, to merge separate video and audio streams.
    pub can_merge: bool,
    active: Arc<Mutex<HashSet<String>>>,
    scratch: PathBuf,
}

/// Holds a user's one download slot until dropped.
pub struct Slot {
    active: Arc<Mutex<HashSet<String>>>,
    user: String,
}

impl Drop for Slot {
    fn drop(&mut self) {
        self.active.lock().unwrap().remove(&self.user);
    }
}

impl Downloader {
    /// Look for yt-dlp and clear out scratch directories left by a crash.
    pub async fn new(program: PathBuf, ffmpeg: PathBuf, data_dir: &Path) -> Self {
        let scratch = data_dir.join("downloads");
        let _ = tokio::fs::remove_dir_all(&scratch).await;
        let version = async {
            let out = tokio::time::timeout(
                Duration::from_secs(15),
                Command::new(&program)
                    .arg("--version")
                    .stdin(Stdio::null())
                    .stderr(Stdio::null())
                    .kill_on_drop(true)
                    .output(),
            )
            .await
            .ok()?
            .ok()?;
            let v = String::from_utf8_lossy(&out.stdout).trim().to_string();
            (out.status.success() && !v.is_empty()).then_some(v)
        }
        .await;
        let can_merge = tokio::time::timeout(
            Duration::from_secs(15),
            Command::new(&ffmpeg)
                .arg("-version")
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .kill_on_drop(true)
                .status(),
        )
        .await
        .ok()
        .and_then(|s| s.ok())
        .is_some_and(|s| s.success());
        Downloader {
            program,
            ffmpeg,
            version,
            can_merge,
            active: Arc::default(),
            scratch,
        }
    }

    pub fn slot(&self, user: &str) -> Result<Slot> {
        if !self.active.lock().unwrap().insert(user.to_string()) {
            return Err(AppError::Busy(
                "you already have a download running; wait for it to finish".into(),
            ));
        }
        Ok(Slot {
            active: self.active.clone(),
            user: user.to_string(),
        })
    }

    async fn scratch_dir(&self) -> Result<ScratchDir> {
        let dir = self.scratch.join(random_token(12));
        tokio::fs::create_dir_all(&dir).await?;
        Ok(ScratchDir(dir))
    }

    fn command(&self, dir: &Path) -> Command {
        let mut c = Command::new(&self.program);
        c.args(COMMON)
            .current_dir(dir)
            .stdin(Stdio::null())
            .kill_on_drop(true);
        c
    }

    /// Look a link up without downloading it.
    pub async fn info(&self, url: &str) -> Result<VideoInfo> {
        let dir = self.scratch_dir().await?;
        let out = tokio::time::timeout(
            Duration::from_secs(60),
            self.command(&dir.0)
                .args(["--dump-single-json", "--", url])
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .output(),
        )
        .await
        .map_err(|_| AppError::bad("the site took too long to answer"))??;
        if !out.status.success() {
            return Err(AppError::bad(yt_dlp_error(&out.stderr)));
        }
        let v: serde_json::Value = serde_json::from_slice(&out.stdout)
            .map_err(|_| AppError::bad("couldn't read what the site sent back"))?;
        Ok(parse_info(&v, self.can_merge))
    }

    /// Start downloading: a stream of the file's bytes.
    pub async fn stream(
        &self,
        url: &str,
        kind: VideoKind,
        max: u64,
        slot: Slot,
    ) -> Result<DownloadStream> {
        let dir = self.scratch_dir().await?;
        let spawn_err = |_| AppError::Unavailable("the downloader couldn't be started".into());
        let mut cmd = self.command(&dir.0);
        match kind {
            VideoKind::Video if self.can_merge => cmd.args(["-f", VIDEO_MERGED]),
            VideoKind::Video => cmd.args(["-f", VIDEO_SINGLE]),
            VideoKind::Audio => cmd.args(["-f", AUDIO_FORMAT]),
        };
        let mut download = cmd
            .args(["--quiet", "-o", "-", "--", url])
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(spawn_err)?;
        let mut children = Vec::new();
        let stdout = if kind == VideoKind::Video && self.can_merge {
            let pipe: Stdio = download
                .stdout
                .take()
                .expect("piped")
                .try_into()
                .map_err(|_| AppError::Internal("pipe".into()))?;
            let mut remux = Command::new(&self.ffmpeg)
                .args(REMUX_ARGS)
                .current_dir(&dir.0)
                .stdin(pipe)
                .stdout(Stdio::piped())
                .stderr(Stdio::null())
                .kill_on_drop(true)
                .spawn()
                .map_err(spawn_err)?;
            let out = remux.stdout.take().expect("piped");
            children.push(download);
            children.push(remux);
            out
        } else {
            let out = download.stdout.take().expect("piped");
            children.push(download);
            out
        };
        // The processes are watched by a task that kills them if the stream
        // is dropped (the browser went away) and reports whether they all
        // succeeded.
        let (cancel_tx, cancel_rx) = oneshot::channel::<()>();
        let (status_tx, status_rx) = oneshot::channel();
        tokio::spawn(async move {
            let ok = tokio::select! {
                ok = async {
                    let mut ok = true;
                    for c in children.iter_mut() {
                        ok &= c.wait().await.is_ok_and(|s| s.success());
                    }
                    ok
                } => ok,
                _ = cancel_rx => false,
            };
            if !ok {
                for c in &mut children {
                    let _ = c.start_kill();
                }
            }
            let _ = status_tx.send(ok);
        });
        Ok(DownloadStream {
            inner: ReaderStream::with_capacity(stdout, 256 * 1024),
            sent: 0,
            max,
            status: Some(status_rx),
            _cancel: cancel_tx,
            _slot: slot,
            _dir: dir,
        })
    }
}

/// A scratch directory, deleted when dropped.
struct ScratchDir(PathBuf);

impl Drop for ScratchDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// yt-dlp's stdout as a body. Ends with an error (which aborts the
/// response) if yt-dlp fails or the file passes the size cap, so the
/// browser never mistakes a partial file for a whole one.
pub struct DownloadStream {
    inner: ReaderStream<ChildStdout>,
    sent: u64,
    max: u64,
    status: Option<oneshot::Receiver<bool>>,
    _cancel: oneshot::Sender<()>,
    _slot: Slot,
    _dir: ScratchDir,
}

impl Stream for DownloadStream {
    type Item = std::io::Result<Bytes>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        if self.status.is_none() {
            return Poll::Ready(None);
        }
        match Pin::new(&mut self.inner).poll_next(cx) {
            Poll::Ready(Some(Ok(bytes))) => {
                self.sent += bytes.len() as u64;
                if self.sent > self.max {
                    self.status = None;
                    return Poll::Ready(Some(Err(std::io::Error::other(
                        "the file is larger than this server allows",
                    ))));
                }
                Poll::Ready(Some(Ok(bytes)))
            }
            Poll::Ready(Some(Err(e))) => {
                self.status = None;
                Poll::Ready(Some(Err(e)))
            }
            Poll::Pending => Poll::Pending,
            // stdout closed: wait for yt-dlp's exit status.
            Poll::Ready(None) => {
                let rx = self.status.as_mut().unwrap();
                match Pin::new(rx).poll(cx) {
                    Poll::Pending => Poll::Pending,
                    Poll::Ready(status) => {
                        self.status = None;
                        match status {
                            Ok(true) if self.sent > 0 => Poll::Ready(None),
                            _ => {
                                Poll::Ready(Some(Err(std::io::Error::other("the download failed"))))
                            }
                        }
                    }
                }
            }
        }
    }
}

/// The last "ERROR:" line yt-dlp printed, without the prefix, for the user.
fn yt_dlp_error(stderr: &[u8]) -> String {
    let text = String::from_utf8_lossy(stderr);
    text.lines()
        .rev()
        .find_map(|l| l.strip_prefix("ERROR: "))
        .map(|l| {
            let l = l.trim();
            let l = if l.chars().count() > 300 {
                l.chars().take(300).collect()
            } else {
                l.to_string()
            };
            if l.contains("Unsupported URL") {
                "that site isn't supported".to_string()
            } else {
                l
            }
        })
        .unwrap_or_else(|| "that link couldn't be downloaded".into())
}

fn parse_info(v: &serde_json::Value, can_merge: bool) -> VideoInfo {
    let s = |k: &str| v.get(k).and_then(|x| x.as_str()).map(str::to_string);
    let formats = v
        .get("formats")
        .and_then(|f| f.as_array())
        .cloned()
        .unwrap_or_default();
    let has = |f: &serde_json::Value, k: &str| {
        f.get(k)
            .and_then(|x| x.as_str())
            .is_some_and(|c| c != "none")
    };
    let size = |f: &serde_json::Value| {
        f.get("filesize")
            .or_else(|| f.get("filesize_approx"))
            .and_then(|x| x.as_f64())
            .map(|x| x as u64)
    };
    let height = |f: &serde_json::Value| f.get("height").and_then(|x| x.as_u64()).map(|h| h as u32);
    let ext = |f: &serde_json::Value| {
        f.get("ext")
            .and_then(|x| x.as_str())
            .unwrap_or("bin")
            .to_string()
    };
    // Mirror VIDEO_FORMAT and AUDIO_FORMAT: yt-dlp lists formats worst to best.
    let pick = |want_video: bool, prefer: &str| {
        let ok: Vec<&serde_json::Value> = formats
            .iter()
            .filter(|f| {
                if want_video {
                    has(f, "vcodec") && has(f, "acodec")
                } else {
                    has(f, "acodec") && !has(f, "vcodec")
                }
            })
            .collect();
        ok.iter()
            .rev()
            .find(|f| ext(f) == prefer)
            .or_else(|| ok.last())
            .map(|f| VideoOption {
                ext: ext(f),
                size: size(f),
                height: if want_video { height(f) } else { None },
            })
    };
    // Mirror VIDEO_MERGED's first choice: H.264 up to 1080p plus AAC.
    let merged = || {
        let video = formats
            .iter()
            .filter(|f| {
                f.get("vcodec")
                    .and_then(|x| x.as_str())
                    .is_some_and(|c| c.starts_with("avc1"))
                    && !has(f, "acodec")
                    && height(f).is_some_and(|h| h <= 1080)
            })
            .max_by_key(|f| height(f))?;
        let audio = formats
            .iter()
            .filter(|f| {
                f.get("acodec")
                    .and_then(|x| x.as_str())
                    .is_some_and(|c| c.starts_with("mp4a"))
                    && !has(f, "vcodec")
            })
            .max_by_key(|f| size(f).unwrap_or(0))?;
        Some(VideoOption {
            ext: "mp4".into(),
            size: size(video).zip(size(audio)).map(|(a, b)| a + b),
            height: height(video),
        })
    };
    VideoInfo {
        title: s("title").unwrap_or_else(|| "video".into()),
        site: s("extractor_key").unwrap_or_default(),
        uploader: s("uploader").or_else(|| s("channel")),
        duration: v.get("duration").and_then(|x| x.as_f64()),
        video: if can_merge {
            merged().or_else(|| pick(true, "mp4"))
        } else {
            pick(true, "mp4")
        },
        audio: pick(false, "m4a"),
    }
}

/// Check a link before yt-dlp sees it: http(s) only, no credentials in it,
/// and (unless `public_only` is off, in tests) a host that resolves only to
/// public addresses.
pub async fn check_link(url: &str, public_only: bool) -> Result<()> {
    let bad = || AppError::bad("enter a link to a video page, starting with https://");
    if url.len() > 2048 || url.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return Err(bad());
    }
    let rest = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))
        .ok_or_else(bad)?;
    let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
    if authority.is_empty() || authority.contains('@') {
        return Err(bad());
    }
    if !public_only {
        return Ok(());
    }
    let (host, port) = split_host_port(authority).ok_or_else(bad)?;
    let addrs: Vec<IpAddr> = tokio::net::lookup_host((host.as_str(), port))
        .await
        .map_err(|_| AppError::bad("that site's address couldn't be found"))?
        .map(|a| a.ip())
        .collect();
    if addrs.is_empty() || addrs.iter().any(|ip| !is_public(ip)) {
        return Err(AppError::bad(
            "links to private or local addresses aren't allowed",
        ));
    }
    Ok(())
}

fn split_host_port(authority: &str) -> Option<(String, u16)> {
    if let Some(rest) = authority.strip_prefix('[') {
        let (host, after) = rest.split_once(']')?;
        let port = match after.strip_prefix(':') {
            Some(p) => p.parse().ok()?,
            None if after.is_empty() => 443,
            None => return None,
        };
        return Some((host.to_string(), port));
    }
    match authority.rsplit_once(':') {
        Some((h, p)) => Some((h.to_string(), p.parse().ok()?)),
        None => Some((authority.to_string(), 443)),
    }
}

fn is_public(ip: &IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            let o = v4.octets();
            !(v4.is_private()
                || v4.is_loopback()
                || v4.is_link_local()
                || v4.is_unspecified()
                || v4.is_broadcast()
                || v4.is_documentation()
                || v4.is_multicast()
                || o[0] == 0
                || (o[0] == 100 && (64..128).contains(&o[1])) // carrier-grade NAT
                || (o[0] == 198 && (o[1] == 18 || o[1] == 19)) // benchmarking
                || o[0] >= 240)
        }
        IpAddr::V6(v6) => {
            if let Some(v4) = v6.to_ipv4_mapped() {
                return is_public(&IpAddr::V4(v4));
            }
            let s = v6.segments();
            !(v6.is_loopback()
                || v6.is_unspecified()
                || v6.is_multicast()
                || (s[0] & 0xfe00) == 0xfc00 // unique local
                || (s[0] & 0xffc0) == 0xfe80 // link local
                || s[0] == 0x2001 && s[1] == 0x0db8) // documentation
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn private_addresses_are_refused() {
        for ip in [
            "127.0.0.1",
            "10.1.2.3",
            "192.168.0.1",
            "172.16.5.5",
            "169.254.169.254",
            "100.64.0.1",
            "0.0.0.0",
            "::1",
            "fd00::1",
            "fe80::1",
            "::ffff:127.0.0.1",
        ] {
            assert!(!is_public(&ip.parse().unwrap()), "{ip}");
        }
        for ip in ["8.8.8.8", "142.250.1.1", "2a00:1450::1"] {
            assert!(is_public(&ip.parse().unwrap()), "{ip}");
        }
    }

    #[test]
    fn host_and_port() {
        assert_eq!(
            split_host_port("example.com"),
            Some(("example.com".into(), 443))
        );
        assert_eq!(
            split_host_port("example.com:8080"),
            Some(("example.com".into(), 8080))
        );
        assert_eq!(split_host_port("[::1]:80"), Some(("::1".into(), 80)));
        assert_eq!(split_host_port("[::1]"), Some(("::1".into(), 443)));
    }
}
