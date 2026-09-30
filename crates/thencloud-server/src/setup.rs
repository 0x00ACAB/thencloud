//! Claiming a new server. The first account becomes the admin, so on a
//! fresh server anyone who found it first could take it (and then make the
//! real owner an admin, none the wiser). So while there are no accounts, the
//! server keeps a setup code in `<data dir>/setup-code` (readable by its own
//! user only) and prints it in its log at start; the first account can only
//! be made with it. `--admin-username` also fixes that account's name.
//!
//! The code only gates creating the account: keys are still made in the
//! browser from the password the admin chooses, which the server never sees.

use std::io::Write;
use std::path::{Path, PathBuf};

use crate::AppState;
use crate::error::{AppError, Result};

const FILE: &str = "setup-code";
const ALPHABET: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";

fn path(data_dir: &Path) -> PathBuf {
    data_dir.join(FILE)
}

/// A new code: 25 characters (125 bits) in groups of five.
fn new_code() -> String {
    let bytes = thencloud_crypto::random_bytes(25);
    let chars: Vec<char> = bytes
        .iter()
        .map(|b| ALPHABET[(b & 31) as usize] as char)
        .collect();
    chars
        .chunks(5)
        .map(|c| c.iter().collect::<String>())
        .collect::<Vec<_>>()
        .join("-")
}

/// Upper case, no dashes or spaces, and O/I/L read as the digits they
/// look like, so a code typed from a log is compared as meant.
fn canonical(code: &str) -> String {
    code.chars()
        .filter(|c| !matches!(c, '-' | ' '))
        .map(|c| match c.to_ascii_uppercase() {
            'O' => '0',
            'I' | 'L' => '1',
            c => c,
        })
        .collect()
}

async fn no_accounts(state: &AppState) -> Result<bool> {
    let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM users")
        .fetch_one(&state.db)
        .await?;
    Ok(n == 0)
}

/// At start: with no accounts yet, make sure there's a setup code (keeping
/// one from an earlier start) and say where to find it. Returns it.
pub async fn prepare(state: &AppState) -> Result<Option<String>> {
    if !no_accounts(state).await? {
        return Ok(None);
    }
    let file = path(&state.config.data_dir);
    let code = match tokio::fs::read_to_string(&file).await {
        Ok(c) if !c.trim().is_empty() => c.trim().to_string(),
        _ => {
            let code = new_code();
            write_private(&file, &code)?;
            code
        }
    };
    let who = match &state.config.admin_username {
        Some(name) => format!(" called {name}"),
        None => String::new(),
    };
    tracing::warn!(
        "No accounts yet. Create the first one{who} (it becomes the admin) with this setup code: {code} \
         (also in {})",
        file.display()
    );
    Ok(Some(code))
}

fn write_private(file: &Path, code: &str) -> Result<()> {
    let mut opts = std::fs::OpenOptions::new();
    opts.write(true).create(true).truncate(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut opts, 0o600);
    let mut f = opts.open(file)?;
    f.write_all(format!("{code}\n").as_bytes())?;
    Ok(())
}

/// Whether making the first account needs a code (the sign-in page asks).
pub fn required(state: &AppState) -> bool {
    path(&state.config.data_dir).exists()
}

/// Check the first account's code and name. Without a setup code file
/// (a server started some other way, as the tests do) only the name is
/// checked.
pub async fn check_first(state: &AppState, username: &str, code: Option<&str>) -> Result<()> {
    if let Some(name) = &state.config.admin_username
        && !name.trim().eq_ignore_ascii_case(username)
    {
        return Err(AppError::SetupRequired(format!(
            "the first account on this server must be called {}",
            name.trim()
        )));
    }
    let Ok(expected) = tokio::fs::read_to_string(path(&state.config.data_dir)).await else {
        return Ok(());
    };
    // Compared as digests, so the time taken says nothing about the code.
    use sha2::{Digest, Sha256};
    let given = canonical(code.unwrap_or_default());
    let ok = !given.is_empty()
        && Sha256::digest(canonical(expected.trim()).as_bytes())
            == Sha256::digest(given.as_bytes());
    if !ok {
        return Err(AppError::SetupRequired(
            "the setup code is wrong; it's in the server's log and its data directory".into(),
        ));
    }
    Ok(())
}

/// The first account exists: the code has done its job.
pub async fn done(state: &AppState) {
    let _ = tokio::fs::remove_file(path(&state.config.data_dir)).await;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codes_read_back_however_they_are_typed() {
        let c = new_code();
        assert_eq!(c.len(), 29);
        assert_eq!(
            canonical(&c),
            canonical(&c.to_lowercase().replace('-', " "))
        );
        assert_eq!(canonical("abcde-0o1il"), "ABCDE00111");
    }
}
