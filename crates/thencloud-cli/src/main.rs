//! `thencloud`: list, download, upload and sync files from the command line.
//! Files are encrypted and decrypted here; sign in with an app password
//! from Settings > App passwords (a read-only one is enough to download).

use std::fs;
use std::io::{self, BufRead, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use serde::{Deserialize, Serialize};
use thencloud_cli::{Client, Error, Result, parse_app_password, safe_name};

#[derive(Parser)]
#[command(name = "thencloud", version, about = "End-to-end encrypted thencloud client")]
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
    Get { remote: String, local: Option<PathBuf> },
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
    let text = fs::read_to_string(&path).map_err(|_| Error::Usage("not signed in; run `thencloud login <server>` first".into()))?;
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
    if let Command::Logout = cmd {
        let _ = fs::remove_file(config_path()?);
        println!("Forgot the app password. Revoke it in Settings > App passwords if nothing else uses it.");
        return Ok(());
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
                    return Err(Error::Usage(format!("{remote} is a folder; use `pull` for folders")));
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
                let existing = client.list(&parent)?.into_iter().find(|e| e.meta.name.eq_ignore_ascii_case(name));
                if existing.as_ref().is_some_and(|e| e.is_folder()) {
                    return Err(Error::Usage(format!("there's a folder called {name} there")));
                }
                client.upload(&local, &parent, name, existing.as_ref())?;
                println!("{}{name}", if existing.is_some() { "new version of " } else { "" });
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
                    return Err(Error::Usage(format!("{} is not a directory", local.display())));
                }
                let folder = client.resolve(&remote)?;
                let s = client.push(&local, &folder, &mut |p| println!("up {p}"))?;
                println!("{} uploaded, {} unchanged", s.transferred, s.unchanged);
            }
            Command::Login { .. } | Command::Logout => unreachable!(),
        }
        Ok(())
    })();
    let _ = client.logout();
    result
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
