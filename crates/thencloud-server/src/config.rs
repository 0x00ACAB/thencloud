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

    /// Directory for the database and encrypted blobs.
    #[arg(long, env = "THENCLOUD_DATA_DIR", default_value = "./data")]
    pub data_dir: PathBuf,

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

    /// Days before items in the trash are deleted permanently.
    #[arg(long, env = "THENCLOUD_TRASH_DAYS", default_value_t = 30)]
    pub trash_days: i64,

    /// How long an unfinished upload is kept, in hours.
    #[arg(long, env = "THENCLOUD_UPLOAD_TTL_HOURS", default_value_t = 24)]
    pub upload_ttl_hours: i64,
}

impl Config {
    /// Settings suitable for tests: everything under `dir`.
    pub fn for_dir(dir: impl Into<PathBuf>) -> Self {
        let dir = dir.into();
        Config {
            bind: "127.0.0.1:0".parse().unwrap(),
            data_dir: dir.join("data"),
            web_dir: dir.join("web"),
            allow_registration: true,
            default_quota: 1024 * 1024 * 1024,
            session_days: 30,
            max_versions: 10,
            trash_days: 30,
            upload_ttl_hours: 24,
        }
    }
}
