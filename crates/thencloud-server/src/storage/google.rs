//! Google Drive, as a place to keep chunks: OAuth (the server's own app, the
//! `drive.file` scope, so only files thencloud made are reachable) and the
//! few Drive calls needed. Only ciphertext is ever sent, under random names
//! in a "thencloud" folder.
//!
//! Requests are blocking (ureq) on the blocking pool, like Turnstile's.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde::Deserialize;

/// Only files this app made or was handed; nothing else in the Drive.
pub const SCOPE: &str = "https://www.googleapis.com/auth/drive.file";
const FOLDER_MIME: &str = "application/vnd.google-apps.folder";

/// Google's endpoints. Tests point these at a stand-in.
#[derive(Debug, Clone)]
pub struct Endpoints {
    pub auth: String,
    pub token: String,
    pub revoke: String,
    pub api: String,
    pub upload: String,
}

impl Default for Endpoints {
    fn default() -> Self {
        Endpoints {
            auth: "https://accounts.google.com/o/oauth2/v2/auth".into(),
            token: "https://oauth2.googleapis.com/token".into(),
            revoke: "https://oauth2.googleapis.com/revoke".into(),
            api: "https://www.googleapis.com".into(),
            upload: "https://www.googleapis.com/upload".into(),
        }
    }
}

#[derive(Debug)]
pub enum DriveError {
    /// The token was refused: the person revoked access, or it expired.
    Unauthorized,
    /// The Drive is full.
    Full,
    NotFound,
    /// Anything else (Google down, rate limits, the network): try again later.
    Other(String),
}

impl std::fmt::Display for DriveError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DriveError::Unauthorized => write!(f, "Google Drive refused the token"),
            DriveError::Full => write!(f, "the Google Drive is full"),
            DriveError::NotFound => write!(f, "not found in Google Drive"),
            DriveError::Other(e) => write!(f, "Google Drive: {e}"),
        }
    }
}

pub type DriveResult<T> = std::result::Result<T, DriveError>;

pub struct Tokens {
    pub access: String,
    pub refresh: String,
    pub expires_in: u64,
}

pub struct About {
    pub email: String,
    /// Free space, or None for no limit.
    pub free: Option<i64>,
}

pub struct Google {
    client_id: String,
    client_secret: String,
    ep: Endpoints,
    agent: ureq::Agent,
    /// Access tokens by account id, until shortly before they expire.
    access: Mutex<HashMap<String, (String, Instant)>>,
}

impl Google {
    /// The server's Google app, if both its id and secret are set.
    pub fn from_config(cfg: &crate::Config) -> Option<Google> {
        let id = cfg.google_client_id.clone().filter(|s| !s.is_empty())?;
        let secret = cfg.google_client_secret.clone().filter(|s| !s.is_empty())?;
        Some(Google {
            client_id: id,
            client_secret: secret,
            ep: cfg.google_endpoints.clone(),
            agent: ureq::Agent::config_builder()
                .http_status_as_error(false)
                .timeout_global(Some(Duration::from_secs(120)))
                .build()
                .into(),
            access: Mutex::default(),
        })
    }

    /// Where to send the browser to ask for access. `prompt=consent` and
    /// offline access, so Google hands out a refresh token every time.
    pub fn auth_url(&self, redirect_uri: &str, state: &str) -> String {
        format!(
            "{}?client_id={}&redirect_uri={}&response_type=code&scope={}&access_type=offline&prompt=consent&include_granted_scopes=false&state={}",
            self.ep.auth,
            enc(&self.client_id),
            enc(redirect_uri),
            enc(SCOPE),
            enc(state)
        )
    }

    /// Trade the code from the redirect for tokens.
    pub async fn exchange(&self, code: &str, redirect_uri: &str) -> DriveResult<Tokens> {
        let form = vec![
            ("code".to_string(), code.to_string()),
            ("client_id".into(), self.client_id.clone()),
            ("client_secret".into(), self.client_secret.clone()),
            ("redirect_uri".into(), redirect_uri.to_string()),
            ("grant_type".into(), "authorization_code".into()),
        ];
        let t: TokenAnswer = self.form(&self.ep.token, form).await?;
        Ok(Tokens {
            access: t.access_token,
            refresh: t
                .refresh_token
                .ok_or_else(|| DriveError::Other("Google gave no refresh token".into()))?,
            expires_in: t.expires_in.unwrap_or(3600),
        })
    }

    /// Remember an access token for an account.
    pub fn keep_access(&self, account_id: &str, access: &str, expires_in: u64) {
        let until = Instant::now() + Duration::from_secs(expires_in.saturating_sub(120));
        self.access
            .lock()
            .unwrap()
            .insert(account_id.to_string(), (access.to_string(), until));
    }

    pub fn forget_access(&self, account_id: &str) {
        self.access.lock().unwrap().remove(account_id);
    }

    /// An access token for the account, from the cache or the refresh token.
    pub async fn access_token(&self, account_id: &str, refresh: &str) -> DriveResult<String> {
        if let Some((t, until)) = self.access.lock().unwrap().get(account_id)
            && Instant::now() < *until
        {
            return Ok(t.clone());
        }
        let form = vec![
            ("refresh_token".to_string(), refresh.to_string()),
            ("client_id".into(), self.client_id.clone()),
            ("client_secret".into(), self.client_secret.clone()),
            ("grant_type".into(), "refresh_token".into()),
        ];
        let t: TokenAnswer = self.form(&self.ep.token, form).await?;
        self.keep_access(account_id, &t.access_token, t.expires_in.unwrap_or(3600));
        Ok(t.access_token)
    }

    /// Give the refresh token back to Google (best effort).
    pub async fn revoke(&self, refresh: &str) {
        let _: DriveResult<serde_json::Value> = self
            .form(&self.ep.revoke, vec![("token".into(), refresh.into())])
            .await;
    }

    /// The account's address and free space.
    pub async fn about(&self, access: &str) -> DriveResult<About> {
        let url = format!(
            "{}/drive/v3/about?fields=user(emailAddress),storageQuota(limit,usage)",
            self.ep.api
        );
        let auth = bearer(access);
        let a: AboutAnswer = self
            .call(move |agent| agent.get(&url).header("Authorization", auth).call())
            .await?;
        let num = |s: &Option<String>| s.as_deref().and_then(|v| v.parse::<i64>().ok());
        let (limit, usage) = (num(&a.storage_quota.limit), num(&a.storage_quota.usage));
        Ok(About {
            email: a.user.email_address,
            free: limit.map(|l| (l - usage.unwrap_or(0)).max(0)),
        })
    }

    /// Make the folder thencloud keeps its chunks in.
    pub async fn create_folder(&self, access: &str) -> DriveResult<String> {
        let url = format!("{}/drive/v3/files?fields=id", self.ep.api);
        let body = serde_json::json!({ "name": "thencloud", "mimeType": FOLDER_MIME }).to_string();
        let auth = bearer(access);
        let f: FileAnswer = self
            .call(move |agent| {
                agent
                    .post(&url)
                    .header("Authorization", auth)
                    .header("Content-Type", "application/json")
                    .send(body.as_bytes())
            })
            .await?;
        Ok(f.id)
    }

    /// Store `data` as a new file in `folder`; its Drive id.
    pub async fn upload(&self, access: &str, folder: &str, data: &[u8]) -> DriveResult<String> {
        let url = format!(
            "{}/drive/v3/files?uploadType=multipart&fields=id",
            self.ep.upload
        );
        let boundary = format!("thencloud-{}", hex(&thencloud_crypto::random_bytes(12)));
        // A random name: Google learns nothing from it, and nothing needs it
        // (the database keeps the file's id).
        let meta = serde_json::json!({
            "name": hex(&thencloud_crypto::random_bytes(16)),
            "parents": [folder],
            "mimeType": "application/octet-stream",
        });
        let mut body = Vec::with_capacity(data.len() + 512);
        body.extend_from_slice(
            format!(
                "--{boundary}\r\nContent-Type: application/json; charset=UTF-8\r\n\r\n{meta}\r\n\
                 --{boundary}\r\nContent-Type: application/octet-stream\r\n\r\n"
            )
            .as_bytes(),
        );
        body.extend_from_slice(data);
        body.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());
        let auth = bearer(access);
        let f: FileAnswer = self
            .call(move |agent| {
                agent
                    .post(&url)
                    .header("Authorization", auth)
                    .header(
                        "Content-Type",
                        format!("multipart/related; boundary={boundary}"),
                    )
                    .send(&body[..])
            })
            .await?;
        Ok(f.id)
    }

    pub async fn download(&self, access: &str, id: &str, limit: u64) -> DriveResult<Vec<u8>> {
        let url = format!("{}/drive/v3/files/{}?alt=media", self.ep.api, enc(id));
        let auth = bearer(access);
        let agent = self.agent.clone();
        blocking(move || {
            let mut res = agent
                .get(&url)
                .header("Authorization", auth)
                .call()
                .map_err(|e| DriveError::Other(e.to_string()))?;
            status(&mut res)?;
            res.body_mut()
                .with_config()
                .limit(limit)
                .read_to_vec()
                .map_err(|e| DriveError::Other(e.to_string()))
        })
        .await
    }

    /// Delete a file; one that's already gone counts as deleted.
    pub async fn delete(&self, access: &str, id: &str) -> DriveResult<()> {
        let url = format!("{}/drive/v3/files/{}", self.ep.api, enc(id));
        let auth = bearer(access);
        let agent = self.agent.clone();
        blocking(move || {
            let mut res = agent
                .delete(&url)
                .header("Authorization", auth)
                .call()
                .map_err(|e| DriveError::Other(e.to_string()))?;
            match status(&mut res) {
                Ok(()) | Err(DriveError::NotFound) => Ok(()),
                Err(e) => Err(e),
            }
        })
        .await
    }

    async fn form<T: for<'de> Deserialize<'de> + Send + 'static>(
        &self,
        url: &str,
        form: Vec<(String, String)>,
    ) -> DriveResult<T> {
        let (agent, url) = (self.agent.clone(), url.to_string());
        blocking(move || {
            let mut res = agent
                .post(&url)
                .send_form(form.iter().map(|(k, v)| (k.as_str(), v.as_str())))
                .map_err(|e| DriveError::Other(e.to_string()))?;
            status(&mut res)?;
            read_json(&mut res)
        })
        .await
    }

    /// Make a request on the blocking pool and read its JSON answer.
    async fn call<T: for<'de> Deserialize<'de> + Send + 'static>(
        &self,
        request: impl FnOnce(&ureq::Agent) -> Result<ureq::http::Response<ureq::Body>, ureq::Error>
        + Send
        + 'static,
    ) -> DriveResult<T> {
        let agent = self.agent.clone();
        blocking(move || {
            let mut res = request(&agent).map_err(|e| DriveError::Other(e.to_string()))?;
            status(&mut res)?;
            read_json(&mut res)
        })
        .await
    }
}

/// Run a blocking request on the blocking pool.
async fn blocking<T: Send + 'static>(
    f: impl FnOnce() -> DriveResult<T> + Send + 'static,
) -> DriveResult<T> {
    tokio::task::spawn_blocking(f)
        .await
        .map_err(|e| DriveError::Other(e.to_string()))?
}

fn status(res: &mut ureq::http::Response<ureq::Body>) -> DriveResult<()> {
    let code = res.status().as_u16();
    if code < 400 {
        return Ok(());
    }
    let body = res
        .body_mut()
        .with_config()
        .limit(64 * 1024)
        .read_to_string()
        .unwrap_or_default();
    Err(match code {
        401 => DriveError::Unauthorized,
        // A refresh token that no longer works.
        400 if body.contains("invalid_grant") => DriveError::Unauthorized,
        403 if body.contains("storageQuotaExceeded") => DriveError::Full,
        404 => DriveError::NotFound,
        _ => DriveError::Other(format!("HTTP {code}")),
    })
}

fn read_json<T: for<'de> Deserialize<'de>>(
    res: &mut ureq::http::Response<ureq::Body>,
) -> DriveResult<T> {
    res.body_mut()
        .with_config()
        .limit(256 * 1024)
        .read_json()
        .map_err(|e| DriveError::Other(e.to_string()))
}

fn bearer(access: &str) -> String {
    format!("Bearer {access}")
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

/// Percent-encode everything but RFC 3986's unreserved characters.
fn enc(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                (b as char).to_string()
            }
            _ => format!("%{b:02X}"),
        })
        .collect()
}

#[derive(Deserialize)]
struct TokenAnswer {
    access_token: String,
    refresh_token: Option<String>,
    expires_in: Option<u64>,
}

#[derive(Deserialize)]
struct FileAnswer {
    id: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct AboutAnswer {
    user: AboutUser,
    storage_quota: Quota,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct AboutUser {
    email_address: String,
}

#[derive(Deserialize)]
struct Quota {
    limit: Option<String>,
    usage: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percent_encoding() {
        assert_eq!(enc("a b/c?d=é"), "a%20b%2Fc%3Fd%3D%C3%A9");
        assert_eq!(enc("A-z.0_9~"), "A-z.0_9~");
    }
}
