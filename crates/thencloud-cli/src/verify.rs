//! Check that a server sends exactly the web client of a release.
//!
//! The browser runs whatever JavaScript the server sends, so a compromised
//! server could send a version that leaks keys. Releases are built
//! reproducibly (`scripts/release-web.sh`) and come with a manifest of every
//! file's SHA-256, signed with minisign. This fetches each file, in each
//! encoding the server offers, and compares.
//!
//! It shows what the server sends to anyone who asks; a server that singles
//! out one browser can't be caught from here.

use std::collections::BTreeMap;
use std::sync::Mutex;
use std::time::Duration;

use serde::Deserialize;
use sha2::{Digest as _, Sha256};

use crate::{Error, Result};

/// minisign public keys of the people who sign releases. Empty until the
/// first signed release; until then pass `--key`.
pub const RELEASE_KEYS: &[&str] = &[];

#[derive(Debug, Deserialize)]
pub struct Manifest {
    pub name: String,
    pub version: String,
    /// Path under the web root -> lowercase hex SHA-256.
    pub files: BTreeMap<String, String>,
}

impl Manifest {
    pub fn parse(bytes: &[u8]) -> Result<Self> {
        let m: Manifest = serde_json::from_slice(bytes)
            .map_err(|e| Error::Usage(format!("not a release manifest: {e}")))?;
        if m.name != "thencloud-web" {
            return Err(Error::Usage("not a thencloud web client manifest".into()));
        }
        for page in ["index.html", "share.html"] {
            if !m.files.contains_key(page) {
                return Err(Error::Usage(format!("the manifest has no {page}")));
            }
        }
        Ok(m)
    }
}

/// Check a minisign signature and return its trusted comment.
/// `public_key` is the base64 line of a minisign public key, or the whole
/// .pub file.
pub fn verify_minisign(message: &[u8], signature: &str, public_key: &str) -> Result<String> {
    let bad = |e: minisign_verify::Error| Error::Usage(format!("signature check failed: {e}"));
    let pk = minisign_verify::PublicKey::decode(public_key)
        .or_else(|_| minisign_verify::PublicKey::from_base64(public_key.trim()))
        .map_err(bad)?;
    let sig = minisign_verify::Signature::decode(signature).map_err(bad)?;
    pk.verify(message, &sig, false).map_err(bad)?;
    Ok(sig.trusted_comment().to_string())
}

#[derive(Debug, Default)]
pub struct Report {
    /// Responses compared (each file in each encoding).
    pub checked: usize,
    pub problems: Vec<String>,
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

fn encode_path(p: &str) -> String {
    p.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' | b'/' => {
                (b as char).to_string()
            }
            _ => format!("%{b:02X}"),
        })
        .collect()
}

/// Where the Turnstile check's script comes from; `/auth` may allow it.
const TURNSTILE: &str = "https://challenges.cloudflare.com";

/// Only same-origin scripts and connections: otherwise verified files could
/// still be joined by code from elsewhere. `/auth` (the Turnstile page, which
/// holds no password or key) may also load Cloudflare's script.
fn check_csp(csp: Option<&str>, turnstile_page: bool) -> Option<String> {
    let Some(csp) = csp else {
        return Some("no Content-Security-Policy header".into());
    };
    let directive = |name: &str| {
        csp.split(';').map(str::trim).find_map(|d| {
            d.strip_prefix(name)
                .map(|v| v.split_whitespace().collect::<Vec<_>>())
        })
    };
    let scripts_ok = match directive("script-src ").as_deref() {
        Some(["'self'"] | ["'self'", "'wasm-unsafe-eval'"]) => true,
        Some(["'self'", t]) => turnstile_page && *t == TURNSTILE,
        _ => false,
    };
    let ok = directive("default-src ") == Some(vec!["'self'"])
        && scripts_ok
        && directive("connect-src ") == Some(vec!["'self'"]);
    (!ok).then(|| format!("the Content-Security-Policy allows more than this server: {csp}"))
}

/// Fetch every file in the manifest from `server` and compare.
pub fn verify_web(server: &str, manifest: &Manifest) -> Result<Report> {
    let base = server.trim_end_matches('/');
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .http_status_as_error(false)
        .max_redirects(0)
        .timeout_global(Some(Duration::from_secs(120)))
        .build()
        .into();
    let fetch =
        |path: &str, encoding: &str| -> std::result::Result<(Vec<u8>, Option<String>), String> {
            let mut res = agent
                .get(format!("{base}{path}"))
                .header("Accept-Encoding", encoding)
                .call()
                .map_err(|e| e.to_string())?;
            if res.status() != 200 {
                return Err(format!("HTTP {}", res.status().as_u16()));
            }
            let csp = res
                .headers()
                .get("content-security-policy")
                .and_then(|v| v.to_str().ok())
                .map(String::from);
            let body = res
                .body_mut()
                .with_config()
                .limit(256 * 1024 * 1024)
                .read_to_vec()
                .map_err(|e| e.to_string())?;
            Ok((body, csp))
        };

    // Each file, plus the two ways into the app: `/` and a share link.
    let mut jobs: Vec<(String, &str)> = manifest
        .files
        .iter()
        .map(|(p, h)| (format!("/{}", encode_path(p)), h.as_str()))
        .collect();
    jobs.push(("/".into(), manifest.files["index.html"].as_str()));
    jobs.push((
        "/s/verify-web".into(),
        manifest.files["share.html"].as_str(),
    ));
    if let Some(auth) = manifest.files.get("auth.html") {
        jobs.push(("/auth".into(), auth.as_str()));
    }

    let report = Mutex::new(Report::default());
    let next = Mutex::new(jobs.iter());
    std::thread::scope(|s| {
        for _ in 0..8 {
            s.spawn(|| {
                loop {
                    let Some((path, want)) = next.lock().unwrap().next() else {
                        break;
                    };
                    for encoding in ["identity", "gzip", "br"] {
                        let result = fetch(path, encoding);
                        let mut r = report.lock().unwrap();
                        r.checked += 1;
                        match result {
                            Ok((body, csp)) => {
                                let got = hex(&Sha256::digest(&body));
                                if got != *want {
                                    r.problems.push(format!(
                                        "{path} ({encoding}): different content (sha256 {got})"
                                    ));
                                }
                                if (path == "/" || path == "/auth") && encoding == "identity" {
                                    r.problems
                                        .extend(check_csp(csp.as_deref(), path == "/auth"));
                                }
                            }
                            Err(e) => r.problems.push(format!("{path} ({encoding}): {e}")),
                        }
                    }
                }
            });
        }
    });
    let mut report = report.into_inner().unwrap();
    report.problems.sort();
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn csp_rules() {
        let ours = "default-src 'self'; script-src 'self' 'wasm-unsafe-eval'; connect-src 'self'; img-src 'self' blob:";
        assert_eq!(check_csp(Some(ours), false), None);
        assert!(check_csp(None, false).is_some());
        assert!(
            check_csp(
                Some(&ours.replace("script-src 'self'", "script-src 'self' https://evil.test")),
                false
            )
            .is_some()
        );
        assert!(check_csp(Some("default-src 'self'; connect-src *"), false).is_some());
        // Cloudflare's script only on /auth.
        let auth = "default-src 'self'; script-src 'self' https://challenges.cloudflare.com; \
                    frame-src https://challenges.cloudflare.com; connect-src 'self'";
        assert_eq!(check_csp(Some(auth), true), None);
        assert!(check_csp(Some(auth), false).is_some());
        assert!(
            check_csp(
                Some(&auth.replace("challenges.cloudflare.com;", "evil.test;")),
                true
            )
            .is_some()
        );
    }

    #[test]
    fn paths_are_encoded() {
        assert_eq!(encode_path("pdfjs/a b.bcmap"), "pdfjs/a%20b.bcmap");
        assert_eq!(encode_path("assets/x-1.js"), "assets/x-1.js");
    }
}
