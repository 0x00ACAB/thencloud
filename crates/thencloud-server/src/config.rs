use std::net::SocketAddr;
use std::path::PathBuf;

use clap::{ArgAction, Parser};

#[derive(Debug, Clone, Parser)]
#[command(
    name = "thencloud-server",
    version,
    about = "Zero-knowledge file cloud server"
)]
pub struct Config {
    /// Address to listen on.
    #[arg(long, env = "THENCLOUD_BIND", default_value = "127.0.0.1:8080")]
    pub bind: SocketAddr,

    /// Directory for the database (and the encrypted blobs, unless an S3
    /// bucket is configured below).
    #[arg(long, env = "THENCLOUD_DATA_DIR", default_value = "./data")]
    pub data_dir: PathBuf,

    /// Endpoint of an S3-compatible blob store (e.g. https://s3.amazonaws.com
    /// or http://127.0.0.1:9000 for MinIO). Set this, the bucket and both
    /// keys to store blobs in S3 instead of the data directory; the database
    /// always stays local. Requests are path-style.
    #[arg(long, env = "THENCLOUD_S3_ENDPOINT")]
    pub s3_endpoint: Option<String>,

    /// S3 region; most S3-compatible servers accept anything.
    #[arg(long, env = "THENCLOUD_S3_REGION", default_value = "us-east-1")]
    pub s3_region: String,

    /// S3 bucket for the blobs.
    #[arg(long, env = "THENCLOUD_S3_BUCKET")]
    pub s3_bucket: Option<String>,

    /// S3 access key.
    #[arg(long, env = "THENCLOUD_S3_ACCESS_KEY")]
    pub s3_access_key: Option<String>,

    /// S3 secret key.
    #[arg(long, env = "THENCLOUD_S3_SECRET_KEY", hide_env_values = true)]
    pub s3_secret_key: Option<String>,

    /// Key prefix inside the bucket (e.g. "thencloud/").
    #[arg(long, env = "THENCLOUD_S3_PREFIX", default_value = "")]
    pub s3_prefix: String,

    /// Directory with the built web client (`web/dist`, see build.sh).
    #[arg(long, env = "THENCLOUD_WEB_DIR", default_value = "./web/dist")]
    pub web_dir: PathBuf,

    /// Allow anyone to register. The first account can always be created
    /// and becomes the admin.
    #[arg(long, env = "THENCLOUD_ALLOW_REGISTRATION", default_value_t = true, action = ArgAction::Set)]
    pub allow_registration: bool,

    /// Storage quota for new users, in bytes of ciphertext.
    #[arg(long, env = "THENCLOUD_DEFAULT_QUOTA", default_value_t = 10 * 1024 * 1024 * 1024)]
    pub default_quota: i64,

    /// Session lifetime in days (sliding).
    #[arg(long, env = "THENCLOUD_SESSION_DAYS", default_value_t = 30)]
    pub session_days: i64,

    /// Versions kept per file, including the current one. Older versions
    /// are deleted when a new one is uploaded.
    #[arg(long, env = "THENCLOUD_MAX_VERSIONS", default_value_t = 10)]
    pub max_versions: i64,

    /// Thin out old versions by age: keep all from the last hour, then one
    /// per hour for a day, one per day for 30 days and one per week after.
    #[arg(long, env = "THENCLOUD_VERSION_THINNING", default_value_t = true, action = ArgAction::Set)]
    pub version_thinning: bool,

    /// Days before items in the trash are deleted permanently.
    #[arg(long, env = "THENCLOUD_TRASH_DAYS", default_value_t = 30)]
    pub trash_days: i64,

    /// How long an unfinished upload is kept, in hours.
    #[arg(long, env = "THENCLOUD_UPLOAD_TTL_HOURS", default_value_t = 24)]
    pub upload_ttl_hours: i64,

    /// The yt-dlp program for the optional video downloader. The downloader
    /// stays off until an admin turns it on in the Admin view.
    #[arg(long, env = "THENCLOUD_YT_DLP", default_value = "yt-dlp")]
    pub yt_dlp: PathBuf,

    /// ffmpeg, used by the video downloader to merge separate video and
    /// audio streams (most YouTube videos). Optional.
    #[arg(long, env = "THENCLOUD_FFMPEG", default_value = "ffmpeg")]
    pub ffmpeg: PathBuf,

    /// Largest video the downloader passes through, in bytes.
    #[arg(long, env = "THENCLOUD_DOWNLOADER_MAX_BYTES", default_value_t = 2 * 1024 * 1024 * 1024)]
    pub downloader_max_bytes: u64,

    /// Behind a reverse proxy: take the client's address from the last
    /// entry of X-Forwarded-For (used only to rate-limit sign-in attempts).
    /// Only turn this on when the proxy sets that header and clients can't
    /// reach the server directly.
    #[arg(long, env = "THENCLOUD_TRUST_PROXY", default_value_t = false, action = ArgAction::Set)]
    pub trust_proxy: bool,

    /// Rate-limit sign-in attempts by the client's address as well as by
    /// account. Turn it off for an onion service, where every visitor comes
    /// from the same address and one could lock everyone out; accounts and
    /// link passwords are still limited.
    #[arg(long, env = "THENCLOUD_LIMIT_BY_ADDRESS", default_value_t = true, action = ArgAction::Set)]
    pub limit_by_address: bool,

    /// Serve Prometheus metrics at /api/metrics to requests that carry
    /// `Authorization: Bearer <this token>`. Off when unset.
    #[arg(long, env = "THENCLOUD_METRICS_TOKEN", hide_env_values = true)]
    pub metrics_token: Option<String>,

    /// Refuse downloader links that point at private or local addresses.
    /// Always on outside tests.
    #[arg(skip = true)]
    pub downloader_public_only: bool,
}

impl Config {
    /// Settings suitable for tests: everything under `dir`.
    pub fn for_dir(dir: impl Into<PathBuf>) -> Self {
        let dir = dir.into();
        Config {
            bind: "127.0.0.1:0".parse().unwrap(),
            data_dir: dir.join("data"),
            web_dir: dir.join("web"),
            s3_endpoint: None,
            s3_region: "us-east-1".into(),
            s3_bucket: None,
            s3_access_key: None,
            s3_secret_key: None,
            s3_prefix: String::new(),
            allow_registration: true,
            default_quota: 1024 * 1024 * 1024,
            session_days: 30,
            max_versions: 10,
            version_thinning: true,
            trash_days: 30,
            upload_ttl_hours: 24,
            yt_dlp: "yt-dlp".into(),
            ffmpeg: "ffmpeg".into(),
            downloader_max_bytes: 2 * 1024 * 1024 * 1024,
            downloader_public_only: true,
            metrics_token: None,
            trust_proxy: false,
            limit_by_address: true,
        }
    }
}
