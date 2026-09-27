//! Client-side cryptography for thencloud.
//!
//! Everything in this crate runs on the *client* (natively or compiled to
//! WASM). The server only ever sees the outputs: ciphertexts, wrapped keys,
//! public keys and the `auth_key` (which it hashes again before storing).
//!
//! Key hierarchy:
//!
//! ```text
//! password ──Argon2id──► root ──HKDF──┬─► auth_key   (sent to server, re-hashed there)
//!                                     └─► kek        (never leaves the client)
//! kek ──wraps──► master key (MK)
//! MK  ──wraps──► X25519 private key, root folder key
//! folder key ──wraps──► child node keys (files & folders)
//! node key ──seals──► node metadata (name, mime, size, mtime)
//! file node key ──wraps──► per-version content key ──seals──► content chunks
//! recipient public key ──sealed box──► shared node key
//! ```
//!
//! Every ciphertext is bound to its context through AEAD associated data
//! (node id, version id, chunk index, ...), so the server cannot swap, move,
//! reorder or truncate ciphertexts without decryption failing.

pub mod api;

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use chacha20poly1305::{XChaCha20Poly1305, XNonce};
use hkdf::Hkdf;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use zeroize::{Zeroize, ZeroizeOnDrop};

pub const KEY_LEN: usize = 32;
pub const NONCE_LEN: usize = 24;
pub const TAG_LEN: usize = 16;
pub const SALT_LEN: usize = 16;
/// Plaintext bytes per content chunk.
pub const CHUNK_SIZE: usize = 4 * 1024 * 1024;
/// Upper bound on the size of one encrypted chunk.
pub const MAX_ENCRYPTED_CHUNK: usize = CHUNK_SIZE + NONCE_LEN + TAG_LEN;

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum Error {
    #[error("decryption failed (wrong key or tampered data)")]
    Decrypt,
    #[error("invalid key length")]
    KeyLength,
    #[error("invalid KDF parameters")]
    KdfParams,
    #[error("invalid base64")]
    Base64,
    #[error("invalid metadata: {0}")]
    Metadata(String),
    #[error("random number generator failed")]
    Rng,
    #[error("that recovery key isn't valid; check it for typos")]
    RecoveryKey,
}

pub type Result<T> = std::result::Result<T, Error>;

/// A 256-bit symmetric key, zeroed on drop.
#[derive(Clone, Zeroize, ZeroizeOnDrop, PartialEq, Eq)]
pub struct Key([u8; KEY_LEN]);

impl Key {
    pub fn generate() -> Self {
        let mut k = [0u8; KEY_LEN];
        fill_random(&mut k);
        Key(k)
    }

    pub fn from_slice(b: &[u8]) -> Result<Self> {
        let arr: [u8; KEY_LEN] = b.try_into().map_err(|_| Error::KeyLength)?;
        Ok(Key(arr))
    }

    pub fn as_bytes(&self) -> &[u8; KEY_LEN] {
        &self.0
    }

    pub fn to_b64(&self) -> String {
        b64_encode(&self.0)
    }

    pub fn from_b64(s: &str) -> Result<Self> {
        Self::from_slice(&b64_decode(s)?)
    }
}

impl std::fmt::Debug for Key {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Key(<redacted>)")
    }
}

pub fn fill_random(buf: &mut [u8]) {
    getrandom::fill(buf).expect("system RNG unavailable");
}

pub fn random_bytes(n: usize) -> Vec<u8> {
    let mut v = vec![0u8; n];
    fill_random(&mut v);
    v
}

/// A random RFC 4122 version 4 UUID. Node and version ids are chosen by the
/// client because they are bound into ciphertexts as associated data.
pub fn new_id() -> String {
    let mut b = [0u8; 16];
    fill_random(&mut b);
    b[6] = (b[6] & 0x0f) | 0x40;
    b[8] = (b[8] & 0x3f) | 0x80;
    let h: String = b.iter().map(|x| format!("{x:02x}")).collect();
    format!(
        "{}-{}-{}-{}-{}",
        &h[0..8],
        &h[8..12],
        &h[12..16],
        &h[16..20],
        &h[20..32]
    )
}

pub fn b64_encode(b: &[u8]) -> String {
    URL_SAFE_NO_PAD.encode(b)
}

pub fn b64_decode(s: &str) -> Result<Vec<u8>> {
    URL_SAFE_NO_PAD
        .decode(s.trim_end_matches('='))
        .map_err(|_| Error::Base64)
}

// ---------------------------------------------------------------------------
// AEAD primitive: XChaCha20-Poly1305, output = nonce || ciphertext || tag
// ---------------------------------------------------------------------------

pub fn seal(key: &Key, plaintext: &[u8], aad: &[u8]) -> Vec<u8> {
    let cipher = XChaCha20Poly1305::new(key.as_bytes().into());
    let mut nonce = [0u8; NONCE_LEN];
    fill_random(&mut nonce);
    let ct = cipher
        .encrypt(
            XNonce::from_slice(&nonce),
            Payload {
                msg: plaintext,
                aad,
            },
        )
        .expect("XChaCha20-Poly1305 encryption cannot fail for in-range inputs");
    let mut out = Vec::with_capacity(NONCE_LEN + ct.len());
    out.extend_from_slice(&nonce);
    out.extend_from_slice(&ct);
    out
}

pub fn open(key: &Key, sealed: &[u8], aad: &[u8]) -> Result<Vec<u8>> {
    if sealed.len() < NONCE_LEN + TAG_LEN {
        return Err(Error::Decrypt);
    }
    let (nonce, ct) = sealed.split_at(NONCE_LEN);
    let cipher = XChaCha20Poly1305::new(key.as_bytes().into());
    cipher
        .decrypt(XNonce::from_slice(nonce), Payload { msg: ct, aad })
        .map_err(|_| Error::Decrypt)
}

fn open_key(key: &Key, sealed: &[u8], aad: &[u8]) -> Result<Key> {
    let mut pt = open(key, sealed, aad)?;
    let k = Key::from_slice(&pt);
    pt.zeroize();
    k
}

// Associated-data labels. Versioned so the formats can evolve.
fn aad(label: &str, parts: &[&str]) -> Vec<u8> {
    let mut v = format!("thencloud/v1/{label}").into_bytes();
    for p in parts {
        v.push(0);
        v.extend_from_slice(p.as_bytes());
    }
    v
}

// ---------------------------------------------------------------------------
// Account keys
// ---------------------------------------------------------------------------

/// Argon2id cost parameters. Stored (in the clear) by the server so the
/// client can re-derive keys on login.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct KdfParams {
    /// Memory in KiB.
    pub m_cost: u32,
    pub t_cost: u32,
    pub p_cost: u32,
}

impl Default for KdfParams {
    fn default() -> Self {
        KdfParams {
            m_cost: 64 * 1024,
            t_cost: 3,
            p_cost: 1,
        }
    }
}

impl KdfParams {
    /// Bounds accepted by the server. The lower bound follows the OWASP
    /// minimum for Argon2id (19 MiB, t=2).
    pub fn is_acceptable(&self) -> bool {
        (19 * 1024..=1024 * 1024).contains(&self.m_cost)
            && (2..=10).contains(&self.t_cost)
            && (1..=4).contains(&self.p_cost)
    }
}

pub struct AccountKeys {
    /// Proves knowledge of the password to the server. Not a secret from
    /// the server's point of view, but it reveals nothing about `kek`.
    pub auth_key: Key,
    /// Key-encryption key protecting the master key. Never leaves the client.
    pub kek: Key,
}

pub fn derive_account_keys(password: &str, salt: &[u8], params: KdfParams) -> Result<AccountKeys> {
    let p = argon2::Params::new(params.m_cost, params.t_cost, params.p_cost, Some(KEY_LEN))
        .map_err(|_| Error::KdfParams)?;
    let a2 = argon2::Argon2::new(argon2::Algorithm::Argon2id, argon2::Version::V0x13, p);
    let mut root = [0u8; KEY_LEN];
    a2.hash_password_into(password.as_bytes(), salt, &mut root)
        .map_err(|_| Error::KdfParams)?;
    let hk = Hkdf::<Sha256>::new(None, &root);
    let mut auth = [0u8; KEY_LEN];
    let mut kek = [0u8; KEY_LEN];
    hk.expand(b"thencloud/v1/auth", &mut auth)
        .expect("valid length");
    hk.expand(b"thencloud/v1/kek", &mut kek)
        .expect("valid length");
    root.zeroize();
    Ok(AccountKeys {
        auth_key: Key(auth),
        kek: Key(kek),
    })
}

pub fn wrap_master_key(kek: &Key, mk: &Key) -> Vec<u8> {
    seal(kek, mk.as_bytes(), &aad("master-key", &[]))
}

pub fn unwrap_master_key(kek: &Key, wrapped: &[u8]) -> Result<Key> {
    open_key(kek, wrapped, &aad("master-key", &[]))
}

// ---------------------------------------------------------------------------
// Recovery keys
//
// An optional second way to unwrap the master key, for when the password is
// forgotten. The key is 32 random bytes, so unlike a password it needs no
// slow KDF: HKDF splits it into an auth key (the server stores only a hash)
// and a KEK that wraps the master key. It's shown to the user as Crockford
// base32 in groups of five, with a 2-byte checksum to catch typos.
// ---------------------------------------------------------------------------

const CROCKFORD: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";

fn recovery_checksum(k: &Key) -> [u8; 2] {
    let h = Sha256::digest([b"thencloud/v1/recovery-check\0".as_slice(), k.as_bytes()].concat());
    [h[0], h[1]]
}

/// "ABCDE-FGHJK-...": the key and its checksum, for writing down.
pub fn encode_recovery_key(k: &Key) -> String {
    let mut bytes = k.as_bytes().to_vec();
    bytes.extend_from_slice(&recovery_checksum(k));
    let (mut out, mut acc, mut bits) = (String::new(), 0u32, 0u32);
    for &b in &bytes {
        acc = (acc << 8) | b as u32;
        bits += 8;
        while bits >= 5 {
            bits -= 5;
            out.push(CROCKFORD[((acc >> bits) & 31) as usize] as char);
        }
    }
    if bits > 0 {
        out.push(CROCKFORD[((acc << (5 - bits)) & 31) as usize] as char);
    }
    bytes.zeroize();
    out.as_bytes()
        .chunks(5)
        .map(|c| std::str::from_utf8(c).unwrap())
        .collect::<Vec<_>>()
        .join("-")
}

/// Parse a typed recovery key. Case, spaces and dashes don't matter, and
/// the look-alikes O/0 and I/L/1 are read the same way.
pub fn decode_recovery_key(s: &str) -> Result<Key> {
    let (mut bytes, mut acc, mut bits) = (Vec::with_capacity(35), 0u32, 0u32);
    for ch in s.chars().filter(|c| !c.is_whitespace() && *c != '-') {
        let c = match ch.to_ascii_uppercase() {
            'O' => '0',
            'I' | 'L' => '1',
            c => c,
        };
        let v = CROCKFORD
            .iter()
            .position(|&x| x as char == c)
            .ok_or(Error::RecoveryKey)?;
        acc = (acc << 5) | v as u32;
        bits += 5;
        if bits >= 8 {
            bits -= 8;
            bytes.push((acc >> bits) as u8);
        }
    }
    if bytes.len() != KEY_LEN + 2 {
        return Err(Error::RecoveryKey);
    }
    let k = Key::from_slice(&bytes[..KEY_LEN])?;
    let ok = recovery_checksum(&k) == bytes[KEY_LEN..];
    bytes.zeroize();
    if ok { Ok(k) } else { Err(Error::RecoveryKey) }
}

/// Split a recovery key into an auth key for the server and a KEK.
pub fn derive_recovery_keys(k: &Key) -> AccountKeys {
    let hk = Hkdf::<Sha256>::new(Some(b"thencloud/v1/recovery"), k.as_bytes());
    let mut auth = [0u8; KEY_LEN];
    let mut kek = [0u8; KEY_LEN];
    hk.expand(b"thencloud/v1/recovery-auth", &mut auth)
        .expect("valid length");
    hk.expand(b"thencloud/v1/recovery-kek", &mut kek)
        .expect("valid length");
    AccountKeys {
        auth_key: Key(auth),
        kek: Key(kek),
    }
}

pub fn wrap_master_key_recovery(kek: &Key, mk: &Key) -> Vec<u8> {
    seal(kek, mk.as_bytes(), &aad("master-key-recovery", &[]))
}

pub fn unwrap_master_key_recovery(kek: &Key, wrapped: &[u8]) -> Result<Key> {
    open_key(kek, wrapped, &aad("master-key-recovery", &[]))
}

// ---------------------------------------------------------------------------
// Asymmetric keys (sharing)
// ---------------------------------------------------------------------------

pub struct KeyPair {
    pub secret: Key,
    pub public: [u8; 32],
}

impl KeyPair {
    pub fn generate() -> Self {
        Self::from_secret(Key::generate())
    }

    pub fn from_secret(secret: Key) -> Self {
        let sk = x25519_dalek::StaticSecret::from(*secret.as_bytes());
        let public = x25519_dalek::PublicKey::from(&sk).to_bytes();
        KeyPair { secret, public }
    }
}

pub fn wrap_private_key(mk: &Key, secret: &Key) -> Vec<u8> {
    seal(mk, secret.as_bytes(), &aad("private-key", &[]))
}

pub fn unwrap_private_key(mk: &Key, wrapped: &[u8]) -> Result<KeyPair> {
    Ok(KeyPair::from_secret(open_key(
        mk,
        wrapped,
        &aad("private-key", &[]),
    )?))
}

/// Human-comparable fingerprint of a public key (first 128 bits of SHA-256,
/// in groups of four hex digits).
pub fn fingerprint(public: &[u8]) -> String {
    let d = Sha256::digest(public);
    d[..16]
        .chunks(2)
        .map(|c| format!("{:02x}{:02x}", c[0], c[1]))
        .collect::<Vec<_>>()
        .join(" ")
}

fn seal_box_key(shared: &[u8; 32], eph_pub: &[u8], recipient_pub: &[u8]) -> Key {
    let mut salt = Vec::with_capacity(64);
    salt.extend_from_slice(eph_pub);
    salt.extend_from_slice(recipient_pub);
    let hk = Hkdf::<Sha256>::new(Some(&salt), shared);
    let mut k = [0u8; KEY_LEN];
    hk.expand(b"thencloud/v1/sealed-box", &mut k)
        .expect("valid length");
    Key(k)
}

/// Anonymous public-key encryption: `eph_pub || seal(k, plaintext)`.
pub fn seal_to_public(recipient_pub: &[u8], plaintext: &[u8], aad: &[u8]) -> Result<Vec<u8>> {
    let rp: [u8; 32] = recipient_pub.try_into().map_err(|_| Error::KeyLength)?;
    let eph = x25519_dalek::StaticSecret::from(*Key::generate().as_bytes());
    let eph_pub = x25519_dalek::PublicKey::from(&eph).to_bytes();
    let shared = eph.diffie_hellman(&x25519_dalek::PublicKey::from(rp));
    let k = seal_box_key(shared.as_bytes(), &eph_pub, &rp);
    let mut out = eph_pub.to_vec();
    out.extend(seal(&k, plaintext, aad));
    Ok(out)
}

pub fn open_sealed(kp: &KeyPair, sealed: &[u8], aad: &[u8]) -> Result<Vec<u8>> {
    if sealed.len() < 32 {
        return Err(Error::Decrypt);
    }
    let (eph_pub, rest) = sealed.split_at(32);
    let eph: [u8; 32] = eph_pub.try_into().unwrap();
    let sk = x25519_dalek::StaticSecret::from(*kp.secret.as_bytes());
    let shared = sk.diffie_hellman(&x25519_dalek::PublicKey::from(eph));
    let k = seal_box_key(shared.as_bytes(), eph_pub, &kp.public);
    open(&k, rest, aad)
}

/// Wrap a node key for a share recipient.
pub fn seal_share_key(recipient_pub: &[u8], node_key: &Key, node_id: &str) -> Result<Vec<u8>> {
    seal_to_public(
        recipient_pub,
        node_key.as_bytes(),
        &aad("share", &[node_id]),
    )
}

pub fn open_share_key(kp: &KeyPair, sealed: &[u8], node_id: &str) -> Result<Key> {
    let mut pt = open_sealed(kp, sealed, &aad("share", &[node_id]))?;
    let k = Key::from_slice(&pt);
    pt.zeroize();
    k
}

// ---------------------------------------------------------------------------
// Node keys and metadata
// ---------------------------------------------------------------------------

/// Wrap a node's key under its parent folder's key (or, for the root
/// folder, under the master key).
pub fn wrap_node_key(parent_key: &Key, node_key: &Key, node_id: &str) -> Vec<u8> {
    seal(
        parent_key,
        node_key.as_bytes(),
        &aad("node-key", &[node_id]),
    )
}

pub fn unwrap_node_key(parent_key: &Key, wrapped: &[u8], node_id: &str) -> Result<Key> {
    open_key(parent_key, wrapped, &aad("node-key", &[node_id]))
}

/// Everything about a node the server must not learn.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Metadata {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mime: Option<String>,
    /// Plaintext size in bytes (0 for folders).
    #[serde(default)]
    pub size: u64,
    /// Modification time, milliseconds since the Unix epoch.
    #[serde(default)]
    pub mtime: i64,
}

impl Metadata {
    pub fn validate(&self) -> Result<()> {
        let n = &self.name;
        if n.is_empty()
            || n.len() > 1024
            || n == "."
            || n == ".."
            || n.contains('/')
            || n.contains('\0')
        {
            return Err(Error::Metadata("invalid name".into()));
        }
        Ok(())
    }
}

pub fn encrypt_metadata(node_key: &Key, node_id: &str, meta: &Metadata) -> Result<Vec<u8>> {
    meta.validate()?;
    let json = serde_json::to_vec(meta).map_err(|e| Error::Metadata(e.to_string()))?;
    Ok(seal(node_key, &json, &aad("metadata", &[node_id])))
}

pub fn decrypt_metadata(node_key: &Key, node_id: &str, sealed: &[u8]) -> Result<Metadata> {
    let json = open(node_key, sealed, &aad("metadata", &[node_id]))?;
    serde_json::from_slice(&json).map_err(|e| Error::Metadata(e.to_string()))
}

// ---------------------------------------------------------------------------
// File content
// ---------------------------------------------------------------------------

pub fn wrap_content_key(
    node_key: &Key,
    content_key: &Key,
    node_id: &str,
    version_id: &str,
) -> Vec<u8> {
    seal(
        node_key,
        content_key.as_bytes(),
        &aad("content-key", &[node_id, version_id]),
    )
}

pub fn unwrap_content_key(
    node_key: &Key,
    wrapped: &[u8],
    node_id: &str,
    version_id: &str,
) -> Result<Key> {
    open_key(
        node_key,
        wrapped,
        &aad("content-key", &[node_id, version_id]),
    )
}

/// Number of chunks for a plaintext of `size` bytes. An empty file is one
/// empty chunk, so truncation to zero chunks is always detectable.
pub fn chunk_count(size: u64) -> u32 {
    size.div_ceil(CHUNK_SIZE as u64).max(1) as u32
}

fn chunk_aad(version_id: &str, index: u32, is_last: bool) -> Vec<u8> {
    let idx = index.to_string();
    aad(
        "chunk",
        &[version_id, &idx, if is_last { "last" } else { "more" }],
    )
}

pub fn encrypt_chunk(
    content_key: &Key,
    version_id: &str,
    index: u32,
    is_last: bool,
    plaintext: &[u8],
) -> Vec<u8> {
    seal(
        content_key,
        plaintext,
        &chunk_aad(version_id, index, is_last),
    )
}

pub fn decrypt_chunk(
    content_key: &Key,
    version_id: &str,
    index: u32,
    is_last: bool,
    sealed: &[u8],
) -> Result<Vec<u8>> {
    open(content_key, sealed, &chunk_aad(version_id, index, is_last))
}

/// Encrypt a whole in-memory buffer into chunks.
pub fn encrypt_content(content_key: &Key, version_id: &str, data: &[u8]) -> Vec<Vec<u8>> {
    let n = chunk_count(data.len() as u64);
    (0..n)
        .map(|i| {
            let start = i as usize * CHUNK_SIZE;
            let end = (start + CHUNK_SIZE).min(data.len());
            encrypt_chunk(content_key, version_id, i, i + 1 == n, &data[start..end])
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fast() -> KdfParams {
        KdfParams {
            m_cost: 19 * 1024,
            t_cost: 2,
            p_cost: 1,
        }
    }

    #[test]
    fn account_keys_roundtrip() {
        let salt = random_bytes(SALT_LEN);
        let a = derive_account_keys("hunter2", &salt, fast()).unwrap();
        let b = derive_account_keys("hunter2", &salt, fast()).unwrap();
        let c = derive_account_keys("hunter3", &salt, fast()).unwrap();
        assert!(a.auth_key == b.auth_key && a.kek == b.kek);
        assert!(a.auth_key != a.kek);
        assert!(a.kek != c.kek);

        let mk = Key::generate();
        let w = wrap_master_key(&a.kek, &mk);
        assert!(unwrap_master_key(&b.kek, &w).unwrap() == mk);
        assert_eq!(unwrap_master_key(&c.kek, &w), Err(Error::Decrypt));
    }

    #[test]
    fn node_key_bound_to_id() {
        let parent = Key::generate();
        let child = Key::generate();
        let id = new_id();
        let w = wrap_node_key(&parent, &child, &id);
        assert!(unwrap_node_key(&parent, &w, &id).unwrap() == child);
        assert!(unwrap_node_key(&parent, &w, &new_id()).is_err());
    }

    #[test]
    fn metadata_roundtrip_and_binding() {
        let k = Key::generate();
        let id = new_id();
        let m = Metadata {
            name: "secret.txt".into(),
            mime: Some("text/plain".into()),
            size: 3,
            mtime: 1,
        };
        let ct = encrypt_metadata(&k, &id, &m).unwrap();
        assert_eq!(decrypt_metadata(&k, &id, &ct).unwrap(), m);
        assert!(decrypt_metadata(&k, &new_id(), &ct).is_err());
        let bad = Metadata {
            name: "a/b".into(),
            ..m
        };
        assert!(encrypt_metadata(&k, &id, &bad).is_err());
    }

    #[test]
    fn sealed_box() {
        let kp = KeyPair::generate();
        let other = KeyPair::generate();
        let nk = Key::generate();
        let id = new_id();
        let s = seal_share_key(&kp.public, &nk, &id).unwrap();
        assert!(open_share_key(&kp, &s, &id).unwrap() == nk);
        assert!(open_share_key(&other, &s, &id).is_err());
        assert!(open_share_key(&kp, &s, &new_id()).is_err());

        let mk = Key::generate();
        let w = wrap_private_key(&mk, &kp.secret);
        assert_eq!(unwrap_private_key(&mk, &w).unwrap().public, kp.public);
        assert_eq!(fingerprint(&kp.public).len(), 39);
    }

    #[test]
    fn chunks_detect_reorder_truncation_and_swap() {
        let ck = Key::generate();
        let v = new_id();
        let data: Vec<u8> = (0..CHUNK_SIZE * 2 + 10).map(|i| i as u8).collect();
        let chunks = encrypt_content(&ck, &v, &data);
        assert_eq!(chunks.len(), 3);

        let mut out = Vec::new();
        for (i, c) in chunks.iter().enumerate() {
            out.extend(decrypt_chunk(&ck, &v, i as u32, i == 2, c).unwrap());
        }
        assert_eq!(out, data);

        // reorder
        assert!(decrypt_chunk(&ck, &v, 0, false, &chunks[1]).is_err());
        // truncation: pretending chunk 1 is the last one
        assert!(decrypt_chunk(&ck, &v, 1, true, &chunks[1]).is_err());
        // chunk from another version
        assert!(decrypt_chunk(&ck, &new_id(), 0, false, &chunks[0]).is_err());
        // bit flip
        let mut t = chunks[0].clone();
        t[40] ^= 1;
        assert!(decrypt_chunk(&ck, &v, 0, false, &t).is_err());
    }

    #[test]
    fn empty_file_is_one_chunk() {
        assert_eq!(chunk_count(0), 1);
        assert_eq!(chunk_count(CHUNK_SIZE as u64), 1);
        assert_eq!(chunk_count(CHUNK_SIZE as u64 + 1), 2);
        let ck = Key::generate();
        let v = new_id();
        let c = encrypt_content(&ck, &v, &[]);
        assert_eq!(c.len(), 1);
        assert!(decrypt_chunk(&ck, &v, 0, true, &c[0]).unwrap().is_empty());
    }

    #[test]
    fn ids_are_uuid_v4() {
        let id = new_id();
        assert_eq!(id.len(), 36);
        assert_eq!(&id[14..15], "4");
        assert_ne!(id, new_id());
    }

    #[test]
    fn recovery_key_round_trip_and_typos() {
        let k = Key::generate();
        let text = encode_recovery_key(&k);
        assert_eq!(text.len(), 55 + 10, "11 groups of 5 with dashes: {text}");
        assert!(decode_recovery_key(&text).unwrap() == k);
        // Case, spacing and look-alike letters don't matter.
        let sloppy = text
            .replace('-', " ")
            .to_lowercase()
            .replace('0', "o")
            .replace('1', "l");
        assert!(decode_recovery_key(&sloppy).unwrap() == k);
        // A single wrong character is caught by the checksum.
        let mut chars: Vec<char> = text.chars().collect();
        chars[3] = if chars[3] == 'A' { 'B' } else { 'A' };
        let typo: String = chars.into_iter().collect();
        assert!(matches!(
            decode_recovery_key(&typo),
            Err(Error::RecoveryKey)
        ));
        assert!(matches!(
            decode_recovery_key("not a key"),
            Err(Error::RecoveryKey)
        ));
        assert!(matches!(
            decode_recovery_key(&text[..50]),
            Err(Error::RecoveryKey)
        ));

        // The derived keys unwrap the master key, and differ from each other.
        let rk = derive_recovery_keys(&k);
        assert!(rk.auth_key != rk.kek);
        let mk = Key::generate();
        let wrapped = wrap_master_key_recovery(&rk.kek, &mk);
        assert!(unwrap_master_key_recovery(&rk.kek, &wrapped).unwrap() == mk);
        // Not interchangeable with the password wrap.
        assert!(unwrap_master_key(&rk.kek, &wrapped).is_err());
        let other = derive_recovery_keys(&Key::generate());
        assert!(unwrap_master_key_recovery(&other.kek, &wrapped).is_err());
    }
}
