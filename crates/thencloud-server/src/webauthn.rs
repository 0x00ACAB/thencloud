//! Just enough WebAuthn to register passkeys and check assertions: client
//! data, authenticator data and COSE public keys (ES256, EdDSA and RS256).
//! Attestation isn't checked; we ask for none and trust nothing from it.
//!
//! The relying party id is the host the passkey was made on (from the
//! client data's origin), stored with it. An assertion must be signed for
//! that id, and the browser only offers a credential to pages on its host,
//! so a passkey made here can't be used through a look-alike site.

use ciborium::Value;
use ring::signature::{self, RsaPublicKeyComponents, UnparsedPublicKey};
use serde::Deserialize;
use sha2::{Digest, Sha256};

use crate::error::{AppError, Result};

const FLAG_UP: u8 = 0x01;
const FLAG_UV: u8 = 0x04;
const FLAG_AT: u8 = 0x40;

#[derive(Deserialize)]
struct ClientData {
    #[serde(rename = "type")]
    kind: String,
    challenge: String,
    origin: String,
}

/// Check the client data and return the host of its origin.
fn client_data(json: &[u8], kind: &str, challenge: &[u8]) -> Result<String> {
    let cd: ClientData =
        serde_json::from_slice(json).map_err(|_| AppError::bad("invalid client data"))?;
    if cd.kind != kind {
        return Err(AppError::bad("unexpected WebAuthn operation"));
    }
    if thencloud_crypto::b64_decode(&cd.challenge).ok().as_deref() != Some(challenge) {
        return Err(AppError::InvalidCredentials);
    }
    origin_host(&cd.origin).ok_or_else(|| AppError::bad("passkeys need https"))
}

/// `https://host[:port]`, or plain http on localhost for development and on
/// an onion service (Tor's own encryption covers it, and Tor Browser treats
/// .onion pages as secure).
fn origin_host(origin: &str) -> Option<String> {
    let (scheme, rest) = origin.split_once("://")?;
    let host = match rest.rsplit_once(':') {
        Some((h, port)) if port.bytes().all(|b| b.is_ascii_digit()) => h,
        _ => rest,
    };
    let lower = host.to_ascii_lowercase();
    let local = lower == "localhost" || lower.ends_with(".localhost") || lower.ends_with(".onion");
    let ok = !host.is_empty()
        && !host.contains(['/', '@', '[', ']'])
        && (scheme == "https" || (scheme == "http" && local));
    ok.then(|| host.to_ascii_lowercase())
}

struct AuthData<'a> {
    rp_id_hash: &'a [u8],
    flags: u8,
    sign_count: u32,
    rest: &'a [u8],
}

fn auth_data(b: &[u8]) -> Result<AuthData<'_>> {
    if b.len() < 37 {
        return Err(AppError::bad("invalid authenticator data"));
    }
    Ok(AuthData {
        rp_id_hash: &b[..32],
        flags: b[32],
        sign_count: u32::from_be_bytes(b[33..37].try_into().unwrap()),
        rest: &b[37..],
    })
}

enum CoseKey {
    Es256 { point: Vec<u8> },
    Ed25519 { x: Vec<u8> },
    Rs256 { n: Vec<u8>, e: Vec<u8> },
}

fn int_key(v: &Value) -> Option<i128> {
    v.as_integer().map(i128::from)
}

impl CoseKey {
    fn parse(bytes: &[u8]) -> Result<Self> {
        let bad = || AppError::bad("unsupported passkey public key");
        let v: Value = ciborium::from_reader(bytes).map_err(|_| bad())?;
        let map = v.as_map().ok_or_else(bad)?;
        let get = |k: i128| {
            map.iter()
                .find(|(key, _)| int_key(key) == Some(k))
                .map(|e| &e.1)
        };
        let int = |k| get(k).and_then(int_key);
        let bytes = |k| get(k).and_then(Value::as_bytes).cloned();
        match (int(1), int(3)) {
            // EC2, ES256, P-256
            (Some(2), Some(-7)) if int(-1) == Some(1) => {
                let (x, y) = (bytes(-2).ok_or_else(bad)?, bytes(-3).ok_or_else(bad)?);
                if x.len() != 32 || y.len() != 32 {
                    return Err(bad());
                }
                Ok(CoseKey::Es256 {
                    point: [&[4u8][..], &x, &y].concat(),
                })
            }
            // OKP, EdDSA, Ed25519
            (Some(1), Some(-8)) if int(-1) == Some(6) => {
                let x = bytes(-2).ok_or_else(bad)?;
                if x.len() != 32 {
                    return Err(bad());
                }
                Ok(CoseKey::Ed25519 { x })
            }
            // RSA, RS256
            (Some(3), Some(-257)) => Ok(CoseKey::Rs256 {
                n: bytes(-1).ok_or_else(bad)?,
                e: bytes(-2).ok_or_else(bad)?,
            }),
            _ => Err(bad()),
        }
    }

    fn verify(&self, msg: &[u8], sig: &[u8]) -> bool {
        match self {
            CoseKey::Es256 { point } => {
                UnparsedPublicKey::new(&signature::ECDSA_P256_SHA256_ASN1, point)
                    .verify(msg, sig)
                    .is_ok()
            }
            CoseKey::Ed25519 { x } => UnparsedPublicKey::new(&signature::ED25519, x)
                .verify(msg, sig)
                .is_ok(),
            CoseKey::Rs256 { n, e } => RsaPublicKeyComponents { n, e }
                .verify(&signature::RSA_PKCS1_2048_8192_SHA256, msg, sig)
                .is_ok(),
        }
    }
}

pub struct NewCredential {
    pub credential_id: Vec<u8>,
    /// COSE_Key, as the authenticator gave it.
    pub public_key: Vec<u8>,
    pub rp_id: String,
    pub sign_count: u32,
}

/// Check a `navigator.credentials.create` response against the challenge
/// we issued.
pub fn register(
    client_data_json: &[u8],
    attestation_object: &[u8],
    challenge: &[u8],
) -> Result<NewCredential> {
    let rp_id = client_data(client_data_json, "webauthn.create", challenge)?;
    let bad = || AppError::bad("invalid attestation");
    let att: Value = ciborium::from_reader(attestation_object).map_err(|_| bad())?;
    let raw = att
        .as_map()
        .and_then(|m| m.iter().find(|(k, _)| k.as_text() == Some("authData")))
        .and_then(|(_, v)| v.as_bytes())
        .ok_or_else(bad)?;
    let ad = auth_data(raw)?;
    if ad.rp_id_hash != Sha256::digest(rp_id.as_bytes()).as_slice() {
        return Err(bad());
    }
    if ad.flags & FLAG_UP == 0 || ad.flags & FLAG_AT == 0 || ad.rest.len() < 18 {
        return Err(bad());
    }
    let len = u16::from_be_bytes([ad.rest[16], ad.rest[17]]) as usize;
    let rest = &ad.rest[18..];
    if len == 0 || len > 1023 || rest.len() < len {
        return Err(bad());
    }
    let (credential_id, mut key) = rest.split_at(len);
    let before = key.len();
    let _: Value = ciborium::from_reader(&mut key).map_err(|_| bad())?;
    let public_key = rest[len..len + before - key.len()].to_vec();
    CoseKey::parse(&public_key)?;
    Ok(NewCredential {
        credential_id: credential_id.to_vec(),
        public_key,
        rp_id,
        sign_count: ad.sign_count,
    })
}

pub struct Stored<'a> {
    pub public_key: &'a [u8],
    pub rp_id: &'a str,
    pub sign_count: u32,
}

/// Check a `navigator.credentials.get` response. Returns the new signature
/// counter. `user_verified` requires the authenticator to have checked a
/// PIN or biometric, not just a touch.
pub fn assert(
    cred: &Stored,
    client_data_json: &[u8],
    authenticator_data: &[u8],
    signature: &[u8],
    challenge: &[u8],
    user_verified: bool,
) -> Result<u32> {
    let host = client_data(client_data_json, "webauthn.get", challenge)?;
    if host != cred.rp_id && !host.ends_with(&format!(".{}", cred.rp_id)) {
        return Err(AppError::InvalidCredentials);
    }
    let ad = auth_data(authenticator_data)?;
    let need = FLAG_UP | if user_verified { FLAG_UV } else { 0 };
    if ad.rp_id_hash != Sha256::digest(cred.rp_id.as_bytes()).as_slice() || ad.flags & need != need
    {
        return Err(AppError::InvalidCredentials);
    }
    let mut msg = authenticator_data.to_vec();
    msg.extend_from_slice(&Sha256::digest(client_data_json));
    if !CoseKey::parse(cred.public_key)?.verify(&msg, signature) {
        return Err(AppError::InvalidCredentials);
    }
    // A counter that doesn't go up means a cloned authenticator. Many
    // passkeys always report zero, which is fine.
    if (ad.sign_count != 0 || cred.sign_count != 0) && ad.sign_count <= cred.sign_count {
        return Err(AppError::InvalidCredentials);
    }
    Ok(ad.sign_count)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn origins() {
        assert_eq!(
            origin_host("https://cloud.example.com").as_deref(),
            Some("cloud.example.com")
        );
        assert_eq!(
            origin_host("https://Cloud.example.com:8443").as_deref(),
            Some("cloud.example.com")
        );
        assert_eq!(
            origin_host("http://localhost:5173").as_deref(),
            Some("localhost")
        );
        assert_eq!(origin_host("http://cloud.example.com"), None);
        assert_eq!(
            origin_host("http://abcdefghijklmnopqrstuvwxyz234567abcdefghijklmnopqrstuv.onion")
                .as_deref(),
            Some("abcdefghijklmnopqrstuvwxyz234567abcdefghijklmnopqrstuv.onion")
        );
        assert_eq!(origin_host("http://evil.onion.example.com"), None);
        assert_eq!(origin_host("https://a@b"), None);
        assert_eq!(origin_host("nonsense"), None);
    }
}
