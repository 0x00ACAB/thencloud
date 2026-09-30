//! `thencloud`: list, download, upload and sync files from the command line.
//! Files are encrypted and decrypted here; sign in with an app password
//! from Settings > App passwords (a read-only one is enough to download).
//! On Linux, `thencloud mount` shows a folder as a drive.

use std::fs;
use std::io::{self, BufRead, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use serde::{Deserialize, Serialize};
#[cfg(target_os = "linux")]
use thencloud_cli::mount::{self, MountOptions};
use thencloud_cli::nextcloud::Nextcloud;
use thencloud_cli::serve::{self, ServeOptions};
use thencloud_cli::{Client, Error, Result, parse_app_password, safe_name, sigstore, verify};
use thencloud_crypto::{self as c, Key};

#[derive(Parser)]
#[command(
    name = "thencloud",
    version,
    about = "End-to-end encrypted thencloud client"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Save the server and an app password (read from THENCLOUD_APP_PASSWORD or stdin).
    Login { server: String },
    /// Forget the saved app password (revoke it in Settings to disable it).
    Logout,
    /// List a folder ("" or "/" is My files).
    Ls {
        #[arg(default_value = "")]
        path: String,
    },
    /// Download a file.
    Get {
        remote: String,
        local: Option<PathBuf>,
    },
    /// Upload a file into a folder (a new version if the name exists).
    Put {
        local: PathBuf,
        #[arg(default_value = "")]
        folder: String,
    },
    /// Make a folder.
    Mkdir { path: String },
    /// Download new and changed files from a folder into a local directory.
    Pull { remote: String, local: PathBuf },
    /// Upload new and changed files from a local directory into a folder.
    Push { local: PathBuf, remote: String },
    /// Save a folder ("" is My files) as one encrypted file, under a new
    /// backup key that's shown once. It can be restored anywhere.
    Backup { remote: String, file: PathBuf },
    /// Copy files from a Nextcloud account into a folder ("" is My files),
    /// encrypting them here. Reads the Nextcloud password (an app password
    /// is best) from NEXTCLOUD_PASSWORD or stdin. Safe to run again: files
    /// already copied are skipped.
    ImportNextcloud {
        /// The Nextcloud address, e.g. https://cloud.example.com.
        server: String,
        /// The Nextcloud user name.
        user: String,
        #[arg(default_value = "")]
        remote: String,
        /// Only this folder on Nextcloud [default: all files].
        #[arg(long, default_value = "")]
        from: String,
    },
    /// Restore a backup into a folder, on this server or any other. Reads
    /// the backup key from THENCLOUD_BACKUP_KEY or stdin.
    Restore { file: PathBuf, remote: String },
    /// Check that a server sends exactly the web client of a signed release.
    VerifyWeb {
        /// The server's address, e.g. https://cloud.example.com.
        server: String,
        /// The release manifest (thencloud-web-<version>.json).
        #[arg(long)]
        manifest: PathBuf,
        /// Its Sigstore bundle, made by the release workflow [default: the
        /// manifest's path + .sigstore.json, if it's there].
        #[arg(long)]
        sigstore: Option<PathBuf>,
        /// The repository whose release workflow must have signed it.
        #[arg(
            long,
            env = "THENCLOUD_RELEASE_REPO",
            default_value = "0x00ACAB/thencloud"
        )]
        repo: String,
        /// Its minisign signature by a maintainer [default: the manifest's
        /// path + .minisig, if it's there].
        #[arg(long)]
        signature: Option<PathBuf>,
        /// The minisign public key the release is signed with.
        #[arg(long, env = "THENCLOUD_RELEASE_KEY")]
        key: Option<String>,
        /// Skip the signatures, to compare with a manifest you built yourself.
        #[arg(long, conflicts_with_all = ["signature", "key", "sigstore"])]
        unsigned: bool,
    },
    /// Serve a folder ("" is My files) over WebDAV on 127.0.0.1, decrypted,
    /// for Finder, Explorer and other file managers. Runs until Ctrl+C.
    Serve {
        #[arg(default_value = "")]
        remote: String,
        /// The port on 127.0.0.1 to listen on.
        #[arg(long, default_value_t = 4918)]
        port: u16,
        /// Refuse all changes (always the case with a read-only app password).
        #[arg(long)]
        read_only: bool,
        /// Keep the secret in the address the same across restarts, so a
        /// mapped drive keeps working (at least 32 characters) [default: a new one each time].
        #[arg(long, env = "THENCLOUD_SERVE_SECRET", hide_env_values = true)]
        secret: Option<String>,
    },
    /// Mount a folder ("" is My files) as a drive with FUSE. Runs until
    /// unmounted with `fusermount3 -u <mountpoint>` or Ctrl+C.
    #[cfg(target_os = "linux")]
    Mount {
        mountpoint: PathBuf,
        #[arg(default_value = "")]
        remote: String,
        /// Refuse all changes (always the case with a read-only app password).
        #[arg(long)]
        read_only: bool,
        /// Let other users on this machine in (needs user_allow_other in /etc/fuse.conf).
        #[arg(long)]
        allow_other: bool,
    },
}

#[derive(Serialize, Deserialize)]
struct Config {
    server: String,
    app_password: String,
}

fn config_path() -> Result<PathBuf> {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
        .ok_or_else(|| Error::Usage("can't find a config directory; set XDG_CONFIG_HOME".into()))?;
    Ok(base.join("thencloud").join("config.json"))
}

fn load_config() -> Result<Config> {
    let path = config_path()?;
    let text = fs::read_to_string(&path)
        .map_err(|_| Error::Usage("not signed in; run `thencloud login <server>` first".into()))?;
    serde_json::from_str(&text).map_err(|e| Error::Usage(format!("{}: {e}", path.display())))
}

/// Written readable by you only: the app password opens your files.
fn save_config(c: &Config) -> Result<()> {
    let path = config_path()?;
    fs::create_dir_all(path.parent().unwrap())?;
    let mut opts = fs::OpenOptions::new();
    opts.write(true).create(true).truncate(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut opts, 0o600);
    let mut f = opts.open(&path)?;
    f.write_all(serde_json::to_string_pretty(c).unwrap().as_bytes())?;
    Ok(())
}

fn device() -> String {
    let host = fs::read_to_string("/etc/hostname").unwrap_or_default();
    match host.trim() {
        "" => "thencloud CLI".into(),
        h => format!("thencloud CLI on {h}"),
    }
}

fn connect() -> Result<Client> {
    let c = load_config()?;
    Client::login(&c.server, &parse_app_password(&c.app_password)?, &device())
}

fn split(path: &str) -> (String, String) {
    let trimmed = path.trim_matches('/');
    match trimmed.rsplit_once('/') {
        Some((dir, name)) => (dir.into(), name.into()),
        None => (String::new(), trimmed.into()),
    }
}

fn run(cmd: Command) -> Result<()> {
    if let Command::Login { server } = &cmd {
        let text = match std::env::var("THENCLOUD_APP_PASSWORD") {
            Ok(t) => t,
            Err(_) => {
                eprint!("App password (from Settings > App passwords): ");
                io::stderr().flush()?;
                let mut line = String::new();
                io::stdin().lock().read_line(&mut line)?;
                line
            }
        };
        let key = parse_app_password(&text)?;
        let client = Client::login(server, &key, &device())?;
        println!("Signed in to {server} as {}", client.me.username);
        client.logout()?;
        return save_config(&Config {
            server: server.clone(),
            app_password: text.trim().into(),
        });
    }
    if let Command::VerifyWeb {
        server,
        manifest,
        sigstore,
        repo,
        signature,
        key,
        unsigned,
    } = &cmd
    {
        return verify_web(
            server,
            manifest,
            Signatures {
                sigstore: sigstore.as_deref(),
                repo,
                minisign: signature.as_deref(),
                key: key.as_deref(),
                unsigned: *unsigned,
            },
        );
    }
    if let Command::Logout = cmd {
        let _ = fs::remove_file(config_path()?);
        println!(
            "Forgot the app password. Revoke it in Settings > App passwords if nothing else uses it."
        );
        return Ok(());
    }

    #[cfg(target_os = "linux")]
    if let Command::Mount {
        mountpoint,
        remote,
        read_only,
        allow_other,
    } = cmd
    {
        let client = connect()?;
        let root = client.resolve(&remote)?;
        if !root.is_folder() {
            let _ = client.logout();
            return Err(Error::Usage(format!("{remote} is a file")));
        }
        eprintln!(
            "Mounting {} at {}. Stop with Ctrl+C or `fusermount3 -u {}`.",
            if remote.trim_matches('/').is_empty() {
                "My files"
            } else {
                &remote
            },
            mountpoint.display(),
            mountpoint.display()
        );
        let opts = MountOptions {
            read_only,
            allow_other,
            temp_dir: std::env::temp_dir(),
        };
        return Ok(mount::mount(client, root, &mountpoint, opts)?);
    }

    if let Command::Serve {
        remote,
        port,
        read_only,
        secret,
    } = cmd
    {
        let secret = match secret {
            Some(s) if s.len() >= 32 && s.bytes().all(|b| b.is_ascii_alphanumeric()) => s,
            Some(_) => {
                return Err(Error::Usage(
                    "THENCLOUD_SERVE_SECRET must be at least 32 letters and digits".into(),
                ));
            }
            None => serve::new_secret(),
        };
        let listener = std::net::TcpListener::bind(("127.0.0.1", port))
            .map_err(|e| Error::Usage(format!("can't listen on 127.0.0.1:{port}: {e}")))?;
        let client = connect()?;
        let root = client.resolve(&remote)?;
        if !root.is_folder() {
            let _ = client.logout();
            return Err(Error::Usage(format!("{remote} is a file")));
        }
        eprintln!(
            "Serving {} at\n\n  http://127.0.0.1:{port}/{secret}/\n\n\
             Files are decrypted on this machine only. Anyone with this address who can\n\
             connect to 127.0.0.1 can read them, so don't share it. Stop with Ctrl+C.",
            if remote.trim_matches('/').is_empty() {
                "My files"
            } else {
                &remote
            },
        );
        let opts = ServeOptions {
            read_only,
            temp_dir: std::env::temp_dir(),
        };
        return Ok(serve::serve(client, root, listener, secret, opts)?);
    }

    let client = connect()?;
    let result = (|| -> Result<()> {
        match cmd {
            Command::Ls { path } => {
                let folder = client.resolve(&path)?;
                if !folder.is_folder() {
                    return Err(Error::Usage(format!("{path} is a file")));
                }
                for e in client.list(&folder)? {
                    if e.is_folder() {
                        println!("{:>12}  {}/", "", e.meta.name);
                    } else {
                        println!("{:>12}  {}", e.meta.size, e.meta.name);
                    }
                }
            }
            Command::Get { remote, local } => {
                let file = client.resolve(&remote)?;
                if file.is_folder() {
                    return Err(Error::Usage(format!(
                        "{remote} is a folder; use `pull` for folders"
                    )));
                }
                let out = local.unwrap_or_else(|| PathBuf::from(safe_name(&file.meta.name)));
                let mut f = fs::File::create(&out)?;
                let n = client.download(&file, &mut f)?;
                println!("{} ({n} bytes)", out.display());
            }
            Command::Put { local, folder } => {
                let parent = client.resolve(&folder)?;
                let name = local
                    .file_name()
                    .and_then(|n| n.to_str())
                    .ok_or_else(|| Error::Usage("give a file to upload".into()))?;
                let existing = client
                    .list(&parent)?
                    .into_iter()
                    .find(|e| e.meta.name.eq_ignore_ascii_case(name));
                if existing.as_ref().is_some_and(|e| e.is_folder()) {
                    return Err(Error::Usage(format!(
                        "there's a folder called {name} there"
                    )));
                }
                client.upload(&local, &parent, name, existing.as_ref())?;
                println!(
                    "{}{name}",
                    if existing.is_some() {
                        "new version of "
                    } else {
                        ""
                    }
                );
            }
            Command::Mkdir { path } => {
                let (dir, name) = split(&path);
                client.mkdir(&client.resolve(&dir)?, &name)?;
            }
            Command::Pull { remote, local } => {
                let folder = client.resolve(&remote)?;
                let s = client.pull(&folder, &local, &mut |p| println!("down {p}"))?;
                println!("{} downloaded, {} unchanged", s.transferred, s.unchanged);
            }
            Command::Push { local, remote } => {
                if !Path::new(&local).is_dir() {
                    return Err(Error::Usage(format!(
                        "{} is not a directory",
                        local.display()
                    )));
                }
                let folder = client.resolve(&remote)?;
                let s = client.push(&local, &folder, &mut |p| println!("up {p}"))?;
                println!("{} uploaded, {} unchanged", s.transferred, s.unchanged);
            }
            Command::Backup { remote, file } => {
                let folder = client.resolve(&remote)?;
                if !folder.is_folder() {
                    return Err(Error::Usage(format!("{remote} is a file")));
                }
                // Never overwrite: the old backup may be the only copy.
                let mut opts = fs::OpenOptions::new();
                opts.write(true).create_new(true);
                #[cfg(unix)]
                std::os::unix::fs::OpenOptionsExt::mode(&mut opts, 0o600);
                let f = opts
                    .open(&file)
                    .map_err(|e| Error::Usage(format!("{}: {e}", file.display())))?;
                let key = Key::generate();
                let s = client.backup(&folder, io::BufWriter::new(f), key.clone(), &mut |p| {
                    println!("saved {p}")
                });
                if let Err(e) = s {
                    let _ = fs::remove_file(&file);
                    return Err(e);
                }
                println!("{} files in {}", s?.transferred, file.display());
                eprintln!(
                    "\nBackup key: {}\nWrite it down. It's the only way to open this backup, and it isn't kept anywhere.",
                    c::encode_recovery_key(&key)
                );
            }
            Command::Restore { file, remote } => {
                let folder = client.resolve(&remote)?;
                if !folder.is_folder() {
                    return Err(Error::Usage(format!("{remote} is a file")));
                }
                let text = match std::env::var("THENCLOUD_BACKUP_KEY") {
                    Ok(t) => t,
                    Err(_) => {
                        eprint!("Backup key: ");
                        io::stderr().flush()?;
                        let mut line = String::new();
                        io::stdin().lock().read_line(&mut line)?;
                        line
                    }
                };
                let key = c::decode_recovery_key(&text).map_err(|_| {
                    Error::Usage("that isn't a backup key; check it for typos".into())
                })?;
                let f = fs::File::open(&file)
                    .map_err(|e| Error::Usage(format!("{}: {e}", file.display())))?;
                let s = client.restore(io::BufReader::new(f), key, &folder, &mut |p| {
                    println!("restored {p}")
                })?;
                println!("{} files restored", s.transferred);
            }
            Command::ImportNextcloud {
                server,
                user,
                remote,
                from,
            } => {
                let folder = client.resolve(&remote)?;
                if !folder.is_folder() {
                    return Err(Error::Usage(format!("{remote} is a file")));
                }
                let password = match std::env::var("NEXTCLOUD_PASSWORD") {
                    Ok(t) => t,
                    Err(_) => {
                        eprint!("Nextcloud password for {user}: ");
                        io::stderr().flush()?;
                        let mut line = String::new();
                        io::stdin().lock().read_line(&mut line)?;
                        line.trim_end_matches(['\r', '\n']).to_string()
                    }
                };
                let nc = Nextcloud::connect(&server, &user, &password)?;
                let s = client
                    .import_nextcloud(&nc, &from, &folder, &mut |p| println!("copied {p}"))?;
                println!("{} copied, {} already here", s.transferred, s.unchanged);
            }
            #[cfg(target_os = "linux")]
            Command::Mount { .. } => unreachable!(),
            Command::Login { .. }
            | Command::Logout
            | Command::VerifyWeb { .. }
            | Command::Serve { .. } => unreachable!(),
        }
        Ok(())
    })();
    let _ = client.logout();
    result
}

/// Which signatures `verify-web` checks the manifest with.
struct Signatures<'a> {
    sigstore: Option<&'a Path>,
    repo: &'a str,
    minisign: Option<&'a Path>,
    key: Option<&'a str>,
    unsigned: bool,
}

/// `explicit`, or `default` if that file exists.
fn given_or_beside(explicit: Option<&Path>, default: PathBuf) -> Option<PathBuf> {
    match explicit {
        Some(p) => Some(p.to_path_buf()),
        None => default.exists().then_some(default),
    }
}

/// Check the manifest's signatures: the release workflow's Sigstore bundle
/// and a maintainer's minisign signature, whichever are there. Every one
/// that's there must check out, and at least one must be.
fn check_signatures(bytes: &[u8], manifest: &Path, version: &str, s: &Signatures) -> Result<()> {
    let mut checked = 0;
    let beside = |ext: &str| PathBuf::from(format!("{}{ext}", manifest.display()));
    if let Some(path) = given_or_beside(s.sigstore, beside(".sigstore.json")) {
        let bundle =
            fs::read(&path).map_err(|e| Error::Usage(format!("{}: {e}", path.display())))?;
        let name = format!(
            "https://github.com/{}/.github/workflows/release.yml@refs/tags/{version}",
            s.repo
        );
        let who = sigstore::Identity {
            name: &name,
            issuer: sigstore::GITHUB_ACTIONS,
        };
        let v = sigstore::verify(&bundle, bytes, &who)?;
        println!(
            "Sigstore: signed by {name}, logged in Rekor (entry {}, at {} UTC)",
            v.log_index,
            utc(v.integrated_time)
        );
        checked += 1;
    }
    if let Some(path) = given_or_beside(s.minisign, beside(".minisig")) {
        let sig = fs::read_to_string(&path)
            .map_err(|e| Error::Usage(format!("{}: {e}", path.display())))?;
        let keys: Vec<&str> = match s.key {
            Some(k) => vec![k],
            None => verify::RELEASE_KEYS.to_vec(),
        };
        if keys.is_empty() {
            return Err(Error::Usage(
                "there's a minisign signature but no key to check it with; pass --key with the signer's minisign public key".into(),
            ));
        }
        let comment = keys
            .iter()
            .find_map(|k| verify::verify_minisign(bytes, &sig, k).ok())
            .ok_or_else(|| {
                Error::Usage("the manifest's minisign signature doesn't check out".into())
            })?;
        println!("minisign: signature good ({comment})");
        checked += 1;
    }
    if checked == 0 {
        return Err(Error::Usage(format!(
            "no signature for {}: put the release's .sigstore.json (or .minisig) next to it, or pass --sigstore",
            manifest.display()
        )));
    }
    Ok(())
}

/// Seconds since the epoch as "2026-09-30 12:00:00".
fn utc(t: i64) -> String {
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
        "{y:04}-{m:02}-{d:02} {:02}:{:02}:{:02}",
        secs / 3600,
        secs / 60 % 60,
        secs % 60
    )
}

fn verify_web(server: &str, manifest: &Path, sigs: Signatures) -> Result<()> {
    let bytes = fs::read(manifest)?;
    let m = verify::Manifest::parse(&bytes)?;
    if !sigs.unsigned {
        check_signatures(&bytes, manifest, &m.version, &sigs)?;
    }
    println!(
        "Checking {} files of thencloud web {} on {server}...",
        m.files.len(),
        m.version
    );
    let report = verify::verify_web(server, &m)?;
    if report.problems.is_empty() {
        println!(
            "All {} responses match: this server sends exactly that release.",
            report.checked
        );
        return Ok(());
    }
    for p in &report.problems {
        println!("  {p}");
    }
    Err(Error::Usage(format!(
        "{} of {} responses don't match the release",
        report.problems.len(),
        report.checked
    )))
}

fn main() -> ExitCode {
    match run(Cli::parse().command) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("thencloud: {e}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::utc;

    #[test]
    fn utc_dates() {
        assert_eq!(utc(0), "1970-01-01 00:00:00");
        assert_eq!(utc(951_782_400), "2000-02-29 00:00:00");
        assert_eq!(utc(1_789_615_987), "2026-09-17 03:33:07");
    }
}
