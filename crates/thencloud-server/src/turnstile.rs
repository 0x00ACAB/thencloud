//! Cloudflare Turnstile: an optional check, before sign-in and open
//! registration, that the request comes from a person. The browser gets a
//! token on `/auth`, a page of its own where Cloudflare's script may run
//! (never the page that holds the password or any key), and sends it with
//! the request; this checks it with Siteverify.
//!
//! Siteverify is sent the secret and the token, nothing else: not the
//! username, and not the client's address.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use axum::http::{HeaderMap, header};
use serde::Deserialize;

use crate::AppState;
use crate::error::{AppError, Result};

pub const SITEVERIFY: &str = "https://challenges.cloudflare.com/turnstile/v0/siteverify";

/// The action `/auth` asks for; one token is good for either sign-in or
/// registration, since both forms are on the same page.
pub const ACTION: &str = "auth";

/// Whether this server asks for Turnstile at all.
pub fn enabled(state: &AppState) -> bool {
    site_key(state).is_some()
}

pub fn site_key(state: &AppState) -> Option<&str> {
    let cfg = &state.config;
    match (&cfg.turnstile_site_key, &cfg.turnstile_secret) {
        (Some(k), Some(s)) if !k.is_empty() && !s.is_empty() => Some(k),
        _ => None,
    }
}

/// Tokens live 300 seconds at Cloudflare; remember used ones for longer.
const USED_FOR: Duration = Duration::from_secs(600);

/// Every token this server has taken, by hash, until it would have expired
/// anyway. Siteverify doesn't reliably refuse a token it has just accepted
/// (two checks 90 ms apart both came back `success`), so a token is marked
/// used here before it's checked, and a second request with it fails.
#[derive(Default)]
pub struct Used(Mutex<HashMap<[u8; 32], Instant>>);

impl Used {
    /// True the first time a token is seen.
    fn first_use(&self, token: &str) -> bool {
        use sha2::{Digest, Sha256};
        let h: [u8; 32] = Sha256::digest(token.as_bytes()).into();
        let mut seen = self.0.lock().unwrap();
        let now = Instant::now();
        seen.retain(|_, at| now.duration_since(*at) < USED_FOR);
        seen.insert(h, now).is_none()
    }
}

#[derive(Deserialize)]
struct Verdict {
    success: bool,
    #[serde(default)]
    action: Option<String>,
    #[serde(default)]
    hostname: Option<String>,
}

/// The host a request was sent to, without the port.
fn request_host(headers: &HeaderMap) -> Option<String> {
    let host = headers.get(header::HOST)?.to_str().ok()?;
    let host = match host.strip_prefix('[') {
        // [::1]:8080
        Some(rest) => rest.split(']').next()?,
        None => host.split(':').next()?,
    };
    Some(host.to_ascii_lowercase())
}

/// Require a good token when Turnstile is on. Fails closed: no token, a
/// token Cloudflare doesn't accept, the wrong action or host, or Siteverify
/// not answering all refuse the request.
pub async fn check(state: &AppState, token: Option<&str>, headers: &HeaderMap) -> Result<()> {
    if !enabled(state) {
        return Ok(());
    }
    let token = token
        .filter(|t| !t.is_empty() && t.len() <= 2048)
        .ok_or(AppError::TurnstileFailed)?
        .to_string();
    if !state.turnstile_used.first_use(&token) {
        return Err(AppError::TurnstileFailed);
    }
    let cfg = &state.config;
    let hosts: Vec<String> = if cfg.turnstile_hostnames.is_empty() {
        request_host(headers).into_iter().collect()
    } else {
        cfg.turnstile_hostnames
            .iter()
            .map(|h| h.trim().to_ascii_lowercase())
            .filter(|h| !h.is_empty())
            .collect()
    };
    let secret = cfg.turnstile_secret.clone().unwrap_or_default();
    let url = cfg.turnstile_verify_url.clone();
    let verdict = tokio::task::spawn_blocking(move || -> std::result::Result<Verdict, String> {
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .timeout_global(Some(Duration::from_secs(10)))
            .build()
            .into();
        let mut res = agent
            .post(&url)
            .send_form([("secret", secret.as_str()), ("response", token.as_str())])
            .map_err(|e| e.to_string())?;
        res.body_mut()
            .with_config()
            .limit(64 * 1024)
            .read_json::<Verdict>()
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| AppError::Internal(e.to_string()))?;
    let verdict = match verdict {
        Ok(v) => v,
        Err(e) => {
            tracing::warn!(error = %e, "Turnstile's Siteverify didn't answer");
            return Err(AppError::TurnstileFailed);
        }
    };
    let host_ok = verdict
        .hostname
        .as_deref()
        .is_some_and(|h| hosts.iter().any(|x| x.eq_ignore_ascii_case(h)));
    if verdict.success && verdict.action.as_deref() == Some(ACTION) && host_ok {
        Ok(())
    } else {
        Err(AppError::TurnstileFailed)
    }
}
