//! The optional video downloader: runs yt-dlp for a link and streams the
//! result straight to the browser, which encrypts and uploads it like any
//! other file.
//!
//! This is the one place the server handles plaintext it didn't get as
//! ciphertext, so it's off by default (an admin enables it) and it keeps as
//! little as possible:
//! - nothing is written to disk: everything goes through pipes. Separate
//!   video and audio streams (most YouTube videos) are fetched by two yt-dlp
//!   processes at full speed, each into a named pipe (FIFO: kernel memory,
//!   not a file on disk), and ffmpeg copies them into one fragmented MP4 on
//!   its stdout. Without ffmpeg only single-file formats are offered. The
//!   processes run in a scratch directory (holding just the FIFOs) that is
//!   deleted afterwards, with yt-dlp's cache and config files turned off;
//! - links are never logged, and the lookup kept in memory for the download
//!   that follows is dropped after ten minutes;
//! - only yt-dlp's site extractors run (its "any URL" generic extractor is
//!   off), and links to private or local addresses are refused, so it
//!   can't be used to reach the server's own network;
//! - one download per user at a time, with a size cap, and every process is
//!   killed as soon as the browser goes away.
//!
//! Why not let yt-dlp merge to stdout itself: it then hands the download to
//! ffmpeg's HTTP client, which sites like YouTube throttle to about playback
//! speed (an 80 MB video took ~110 s instead of ~6 s).

use std::collections::{HashMap, HashSet};
use std::ffi::CString;
use std::future::Future;
use std::net::IpAddr;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::process::Stdio;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};
use std::time::{Duration, Instant};

use axum::body::Bytes;
use futures_core::Stream;
use thencloud_crypto::api::{VideoInfo, VideoKind, VideoOption};
use tokio::process::{Child, ChildStdout, Command};
use tokio::sync::oneshot;
use tokio::task::JoinHandle;
use tokio_util::io::ReaderStream;

use crate::error::{AppError, Result};
use crate::util::random_token;

/// Arguments used for every yt-dlp run.
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

/// Highest video resolution fetched.
const MAX_HEIGHT: u64 = 1080;
/// How long a lookup is kept for the download that follows it.
const PLAN_TTL: Duration = Duration::from_secs(600);

/// ffmpeg output: streams copied (not re-encoded) into a fragmented MP4,
/// which can be written to a pipe and plays almost everywhere.
const MP4_OUT: &[&str] = &[
    "-c",
    "copy",
    "-f",
    "mp4",
    "-movflags",
    "+frag_keyframe+empty_moov+default_base_moof",
    "pipe:1",
];

/// What to fetch for one choice (video, or audio only).
#[derive(Debug, Clone, PartialEq)]
enum Plan {
    /// One format, sent as it comes.
    Direct(String),
    /// One format with video and sound in a container browsers don't play
    /// from a pipe well (HLS gives MPEG-TS): repackaged into MP4.
    Remux { id: String, aac: bool },
    /// Separate video and audio formats, fetched in parallel and merged.
    Merge { video: String, audio: String },
}

#[derive(Clone)]
struct Planned {
    at: Instant,
    video: Option<Plan>,
    audio: Option<Plan>,
}

pub struct Downloader {
    program: PathBuf,
    ffmpeg: PathBuf,
    /// yt-dlp's version, if it was found at startup.
    pub version: Option<String>,
    /// Whether ffmpeg is there, to merge separate video and audio streams.
    pub can_merge: bool,
    active: Arc<Mutex<HashSet<String>>>,
    /// Lookups by (user, link), so the download fetches what was shown.
    plans: Mutex<HashMap<(String, String), Planned>>,
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
    /// Look for yt-dlp and ffmpeg, and clear out scratch directories left
    /// by a crash.
    pub async fn new(program: PathBuf, ffmpeg: PathBuf, data_dir: &Path) -> Self {
        let scratch = data_dir.join("downloads");
        let _ = tokio::fs::remove_dir_all(&scratch).await;
        let run = |p: &Path, arg: &str| {
            let mut c = Command::new(p);
            c.arg(arg)
                .stdin(Stdio::null())
                .stderr(Stdio::null())
                .kill_on_drop(true);
            c
        };
        let version = async {
            let out =
                tokio::time::timeout(Duration::from_secs(15), run(&program, "--version").output())
                    .await
                    .ok()?
                    .ok()?;
            let v = String::from_utf8_lossy(&out.stdout).trim().to_string();
            (out.status.success() && !v.is_empty()).then_some(v)
        }
        .await;
        let can_merge = tokio::time::timeout(
            Duration::from_secs(15),
            run(&ffmpeg, "-version").stdout(Stdio::null()).status(),
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
            plans: Mutex::default(),
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

    fn yt_dlp(&self, dir: &Path) -> Command {
        let mut c = Command::new(&self.program);
        c.args(COMMON)
            .current_dir(dir)
            .stdin(Stdio::null())
            .kill_on_drop(true);
        c
    }

    /// Look a link up without downloading it, and remember what the
    /// download would fetch.
    pub async fn info(&self, user: &str, url: &str) -> Result<VideoInfo> {
        let dir = self.scratch_dir().await?;
        let out = tokio::time::timeout(
            Duration::from_secs(60),
            self.yt_dlp(&dir.0)
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
        let (info, video, audio) = parse_info(&v, self.can_merge);
        let mut plans = self.plans.lock().unwrap();
        plans.retain(|_, p| p.at.elapsed() < PLAN_TTL);
        plans.insert(
            (user.to_string(), url.to_string()),
            Planned {
                at: Instant::now(),
                video,
                audio,
            },
        );
        Ok(info)
    }

    /// What to fetch: from the lookup, or a fresh one if it expired.
    async fn plan(&self, user: &str, url: &str, kind: VideoKind) -> Result<Plan> {
        let key = (user.to_string(), url.to_string());
        let cached = {
            let plans = self.plans.lock().unwrap();
            plans
                .get(&key)
                .filter(|p| p.at.elapsed() < PLAN_TTL)
                .cloned()
        };
        let planned = match cached {
            Some(p) => p,
            None => {
                self.info(user, url).await?;
                self.plans
                    .lock()
                    .unwrap()
                    .get(&key)
                    .cloned()
                    .ok_or(AppError::NotFound)?
            }
        };
        match kind {
            VideoKind::Video => planned.video,
            VideoKind::Audio => planned.audio,
        }
        .ok_or_else(|| AppError::bad("that isn't available for this video"))
    }

    /// Start downloading: a stream of the file's bytes.
    pub async fn stream(
        &self,
        user: &str,
        url: &str,
        kind: VideoKind,
        max: u64,
        slot: Slot,
    ) -> Result<DownloadStream> {
        let plan = self.plan(user, url, kind).await?;
        let dir = self.scratch_dir().await?;
        let spawn_err = |_| AppError::Unavailable("the downloader couldn't be started".into());
        let fetch = |id: &str| {
            let mut c = self.yt_dlp(&dir.0);
            c.args(["--quiet", "-f", id, "-o", "-", "--", url])
                .stdout(Stdio::piped())
                .stderr(Stdio::null());
            c
        };
        let ffmpeg = |inputs: &[&str], extra: &[&str]| {
            let mut c = Command::new(&self.ffmpeg);
            c.args(["-hide_banner", "-loglevel", "error", "-nostdin"]);
            for i in inputs {
                c.args(["-i", i]);
            }
            c.args(extra)
                .args(MP4_OUT)
                .current_dir(&dir.0)
                .stdout(Stdio::piped())
                .stderr(Stdio::null())
                .kill_on_drop(true);
            c
        };

        // `main` is the process whose output is sent; `inputs` feed it.
        let mut inputs: Vec<Child> = Vec::new();
        let mut tasks: Vec<JoinHandle<bool>> = Vec::new();
        let (mut main, stdout) = match &plan {
            Plan::Direct(id) => {
                let mut dl = fetch(id).spawn().map_err(spawn_err)?;
                let out = dl.stdout.take().expect("piped");
                (dl, out)
            }
            Plan::Remux { id, aac } => {
                let mut dl = fetch(id).spawn().map_err(spawn_err)?;
                let pipe: Stdio = dl
                    .stdout
                    .take()
                    .expect("piped")
                    .try_into()
                    .map_err(|_| AppError::Internal("pipe".into()))?;
                let mut extra = vec!["-map", "0:v:0?", "-map", "0:a:0?"];
                if *aac {
                    extra.extend(["-bsf:a", "aac_adtstoasc"]);
                }
                let mut ff = ffmpeg(&["pipe:0"], &extra)
                    .stdin(pipe)
                    .spawn()
                    .map_err(spawn_err)?;
                let out = ff.stdout.take().expect("piped");
                inputs.push(dl);
                (ff, out)
            }
            Plan::Merge { video, audio } => {
                let v_fifo = dir.0.join("video");
                let a_fifo = dir.0.join("audio");
                make_fifo(&v_fifo)?;
                make_fifo(&a_fifo)?;
                let mut ff = ffmpeg(
                    &[path_str(&v_fifo)?, path_str(&a_fifo)?],
                    &["-map", "0:v:0", "-map", "1:a:0"],
                )
                .stdin(Stdio::null())
                .spawn()
                .map_err(spawn_err)?;
                let out = ff.stdout.take().expect("piped");
                for (id, fifo) in [(video, v_fifo), (audio, a_fifo)] {
                    let mut dl = fetch(id).spawn().map_err(spawn_err)?;
                    let from = dl.stdout.take().expect("piped");
                    tasks.push(tokio::spawn(feed_fifo(from, fifo)));
                    inputs.push(dl);
                }
                (ff, out)
            }
        };

        // Everything is watched by a task that kills it all if the stream is
        // dropped (the browser went away) or `main` fails, and reports
        // whether it all succeeded.
        let (cancel_tx, cancel_rx) = oneshot::channel::<()>();
        let (status_tx, status_rx) = oneshot::channel();
        tokio::spawn(async move {
            let ok = tokio::select! {
                ok = async {
                    if !main.wait().await.is_ok_and(|s| s.success()) {
                        return false;
                    }
                    let mut ok = true;
                    for t in tasks.iter_mut() {
                        ok &= t.await.unwrap_or(false);
                    }
                    for c in inputs.iter_mut() {
                        ok &= c.wait().await.is_ok_and(|s| s.success());
                    }
                    ok
                } => ok,
                _ = cancel_rx => false,
            };
            if !ok {
                for t in &tasks {
                    t.abort();
                }
                let _ = main.start_kill();
                for c in &mut inputs {
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

fn path_str(p: &Path) -> Result<&str> {
    p.to_str()
        .ok_or_else(|| AppError::Internal("scratch path".into()))
}

/// A named pipe: data written to it stays in kernel memory until read.
fn make_fifo(path: &Path) -> Result<()> {
    let c = CString::new(path.as_os_str().as_bytes())
        .map_err(|_| AppError::Internal("fifo path".into()))?;
    // SAFETY: `c` is a valid NUL-terminated path for the duration of the call.
    if unsafe { libc::mkfifo(c.as_ptr(), 0o600) } != 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    Ok(())
}

/// Copy one yt-dlp's output into a FIFO that ffmpeg reads. The FIFO can
/// only be opened for writing once ffmpeg has opened it for reading, so
/// this retries (asynchronously, so it can be cancelled) until then.
async fn feed_fifo(mut from: ChildStdout, fifo: PathBuf) -> bool {
    let mut to = loop {
        match tokio::net::unix::pipe::OpenOptions::new().open_sender(&fifo) {
            Ok(s) => break s,
            Err(e) if e.raw_os_error() == Some(libc::ENXIO) => {
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
            Err(_) => return false,
        }
    };
    tokio::io::copy(&mut from, &mut to).await.is_ok()
}

/// A scratch directory, deleted when dropped.
struct ScratchDir(PathBuf);

impl Drop for ScratchDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// The download as a body. Ends with an error (which aborts the response)
/// if anything fails or the file passes the size cap, so the browser never
/// mistakes a partial file for a whole one.
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
            // Output closed: wait until every process has exited.
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

/// What a lookup shows, and the plans behind the video and audio choices.
fn parse_info(v: &serde_json::Value, can_merge: bool) -> (VideoInfo, Option<Plan>, Option<Plan>) {
    type F = serde_json::Value;
    let s = |k: &str| v.get(k).and_then(|x| x.as_str()).map(str::to_string);
    let formats: Vec<F> = v
        .get("formats")
        .and_then(|f| f.as_array())
        .cloned()
        .unwrap_or_default();
    let text = |f: &F, k: &str| f.get(k).and_then(|x| x.as_str()).unwrap_or("").to_string();
    let num = |f: &F, k: &str| f.get(k).and_then(|x| x.as_f64()).unwrap_or(0.0);
    let has_v = |f: &F| !matches!(text(f, "vcodec").as_str(), "" | "none");
    let has_a = |f: &F| !matches!(text(f, "acodec").as_str(), "" | "none");
    let id = |f: &F| text(f, "format_id");
    let height = |f: &F| f.get("height").and_then(|x| x.as_u64());
    let size = |f: &F| {
        f.get("filesize")
            .or_else(|| f.get("filesize_approx"))
            .and_then(|x| x.as_f64())
            .map(|x| x as u64)
    };
    // Plain HTTP(S) files download fastest; HLS and DASH fragments work too.
    let direct = |f: &F| text(f, "protocol").starts_with("http");
    let aac = |f: &F| text(f, "acodec").starts_with("mp4a");
    let fits = |f: &F| height(f).is_none_or(|h| h <= MAX_HEIGHT);
    // Formats come worst to best; `best` keeps the last of the top score.
    fn best<'a, K: PartialOrd>(
        it: impl Iterator<Item = &'a F>,
        key: impl Fn(&F) -> K,
    ) -> Option<&'a F> {
        it.fold(None, |acc: Option<&F>, f| match acc {
            Some(a) if key(a) > key(f) => Some(a),
            _ => Some(f),
        })
    }
    let usable = |f: &&F| {
        !id(f).is_empty()
            && id(f)
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || "-_.".contains(c))
    };

    let mut video = None;
    let mut video_plan = None;
    if can_merge {
        // Separate streams: H.264 first (plays everywhere), then the tallest,
        // then plain HTTP, then bitrate. AAC audio first, then bitrate.
        let v_only = best(
            formats
                .iter()
                .filter(usable)
                .filter(|f| has_v(f) && !has_a(f) && fits(f)),
            |f| {
                (
                    text(f, "vcodec").starts_with("avc1"),
                    height(f).unwrap_or(0),
                    direct(f),
                    num(f, "tbr"),
                )
            },
        );
        let a_only = best(
            formats
                .iter()
                .filter(usable)
                .filter(|f| has_a(f) && !has_v(f)),
            |f| (aac(f), direct(f), num(f, "abr").max(num(f, "tbr"))),
        );
        if let (Some(fv), Some(fa)) = (v_only, a_only) {
            video = Some(VideoOption {
                ext: "mp4".into(),
                size: size(fv).zip(size(fa)).map(|(a, b)| a + b),
                height: height(fv).map(|h| h as u32),
            });
            video_plan = Some(Plan::Merge {
                video: id(fv),
                audio: id(fa),
            });
        }
    }
    if video.is_none() {
        // One file with both: MP4 or WebM over HTTP as is; anything else
        // (e.g. HLS, which arrives as MPEG-TS) repackaged, if ffmpeg is here.
        let plays_as_is = |f: &F| direct(f) && matches!(text(f, "ext").as_str(), "mp4" | "webm");
        let muxed = best(
            formats
                .iter()
                .filter(usable)
                .filter(|f| has_v(f) && has_a(f) && fits(f) && (can_merge || plays_as_is(f))),
            |f| {
                (
                    plays_as_is(f),
                    text(f, "ext") == "mp4",
                    height(f).unwrap_or(0),
                    num(f, "tbr"),
                )
            },
        );
        if let Some(f) = muxed {
            let as_is = plays_as_is(f);
            video = Some(VideoOption {
                ext: if as_is { text(f, "ext") } else { "mp4".into() },
                size: size(f),
                height: height(f).map(|h| h as u32),
            });
            video_plan = Some(if as_is {
                Plan::Direct(id(f))
            } else {
                Plan::Remux {
                    id: id(f),
                    aac: aac(f),
                }
            });
        }
    }

    // Audio: an M4A or WebM file as is, preferring AAC, else repackaged.
    let a_direct = best(
        formats.iter().filter(usable).filter(|f| {
            has_a(f)
                && !has_v(f)
                && direct(f)
                && matches!(
                    text(f, "ext").as_str(),
                    "m4a" | "webm" | "mp3" | "ogg" | "opus"
                )
        }),
        |f| (aac(f), num(f, "abr").max(num(f, "tbr"))),
    );
    let (audio, audio_plan) = match a_direct {
        Some(f) => (
            Some(VideoOption {
                ext: text(f, "ext"),
                size: size(f),
                height: None,
            }),
            Some(Plan::Direct(id(f))),
        ),
        None if can_merge => match best(
            formats
                .iter()
                .filter(usable)
                .filter(|f| has_a(f) && !has_v(f)),
            |f| (aac(f), num(f, "abr")),
        ) {
            Some(f) => (
                Some(VideoOption {
                    ext: "m4a".into(),
                    size: size(f),
                    height: None,
                }),
                Some(Plan::Remux {
                    id: id(f),
                    aac: aac(f),
                }),
            ),
            None => (None, None),
        },
        None => (None, None),
    };

    let info = VideoInfo {
        title: s("title").unwrap_or_else(|| "video".into()),
        site: s("extractor_key").unwrap_or_default(),
        uploader: s("uploader").or_else(|| s("channel")),
        duration: v.get("duration").and_then(|x| x.as_f64()),
        video,
        audio,
    };
    (info, video_plan, audio_plan)
}

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
