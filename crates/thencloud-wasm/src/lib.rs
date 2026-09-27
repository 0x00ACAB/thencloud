//! Thin `wasm-bindgen` layer over `thencloud-crypto` for the browser client.
//!
//! Keys cross the JS boundary as `Uint8Array`s. JS only does networking; all
//! cryptography happens here.

use thencloud_crypto as c;
use wasm_bindgen::prelude::*;

type R<T> = Result<T, JsError>;

fn key(b: &[u8]) -> R<c::Key> {
    Ok(c::Key::from_slice(b)?)
}

fn keypair(secret: &[u8]) -> R<c::KeyPair> {
    Ok(c::KeyPair::from_secret(key(secret)?))
}

#[wasm_bindgen]
pub fn chunk_size() -> usize {
    c::CHUNK_SIZE
}

#[wasm_bindgen]
pub fn chunk_count(size: f64) -> u32 {
    c::chunk_count(size as u64)
}

#[wasm_bindgen]
pub fn new_id() -> String {
    c::new_id()
}

#[wasm_bindgen]
pub fn random_key() -> Vec<u8> {
    c::Key::generate().as_bytes().to_vec()
}

#[wasm_bindgen]
pub fn random_salt() -> Vec<u8> {
    c::random_bytes(c::SALT_LEN)
}

#[wasm_bindgen]
pub fn b64_encode(b: &[u8]) -> String {
    c::b64_encode(b)
}

#[wasm_bindgen]
pub fn b64_decode(s: &str) -> R<Vec<u8>> {
    Ok(c::b64_decode(s)?)
}

/// Default KDF parameters as a JSON string.
#[wasm_bindgen]
pub fn default_kdf_params() -> String {
    serde_json::to_string(&c::KdfParams::default()).unwrap()
}

#[wasm_bindgen]
pub struct AccountKeys {
    auth_key: Vec<u8>,
    kek: Vec<u8>,
}

#[wasm_bindgen]
impl AccountKeys {
    #[wasm_bindgen(getter)]
    pub fn auth_key(&self) -> Vec<u8> {
        self.auth_key.clone()
    }
    #[wasm_bindgen(getter)]
    pub fn kek(&self) -> Vec<u8> {
        self.kek.clone()
    }
}

/// `params_json` is a JSON-encoded `KdfParams` (as returned by prelogin).
#[wasm_bindgen]
pub fn derive_account_keys(password: &str, salt: &[u8], params_json: &str) -> R<AccountKeys> {
    let params: c::KdfParams = serde_json::from_str(params_json)?;
    let k = c::derive_account_keys(password, salt, params)?;
    Ok(AccountKeys {
        auth_key: k.auth_key.as_bytes().to_vec(),
        kek: k.kek.as_bytes().to_vec(),
    })
}

#[wasm_bindgen]
pub fn wrap_master_key(kek: &[u8], mk: &[u8]) -> R<Vec<u8>> {
    Ok(c::wrap_master_key(&key(kek)?, &key(mk)?))
}

#[wasm_bindgen]
pub fn unwrap_master_key(kek: &[u8], wrapped: &[u8]) -> R<Vec<u8>> {
    Ok(c::unwrap_master_key(&key(kek)?, wrapped)?
        .as_bytes()
        .to_vec())
}

#[wasm_bindgen]
pub struct KeyPair {
    secret: Vec<u8>,
    public: Vec<u8>,
}

#[wasm_bindgen]
impl KeyPair {
    #[wasm_bindgen(getter)]
    pub fn secret(&self) -> Vec<u8> {
        self.secret.clone()
    }
    #[wasm_bindgen(getter)]
    pub fn public(&self) -> Vec<u8> {
        self.public.clone()
    }
}

#[wasm_bindgen]
pub fn generate_keypair() -> KeyPair {
    let kp = c::KeyPair::generate();
    KeyPair {
        secret: kp.secret.as_bytes().to_vec(),
        public: kp.public.to_vec(),
    }
}

#[wasm_bindgen]
pub fn wrap_private_key(mk: &[u8], secret: &[u8]) -> R<Vec<u8>> {
    Ok(c::wrap_private_key(&key(mk)?, &key(secret)?))
}

/// Returns the raw X25519 secret key.
#[wasm_bindgen]
pub fn unwrap_private_key(mk: &[u8], wrapped: &[u8]) -> R<Vec<u8>> {
    Ok(c::unwrap_private_key(&key(mk)?, wrapped)?
        .secret
        .as_bytes()
        .to_vec())
}

#[wasm_bindgen]
pub fn public_key_from_secret(secret: &[u8]) -> R<Vec<u8>> {
    Ok(keypair(secret)?.public.to_vec())
}

#[wasm_bindgen]
pub fn fingerprint(public: &[u8]) -> String {
    c::fingerprint(public)
}

#[wasm_bindgen]
pub fn wrap_node_key(parent_key: &[u8], node_key: &[u8], node_id: &str) -> R<Vec<u8>> {
    Ok(c::wrap_node_key(
        &key(parent_key)?,
        &key(node_key)?,
        node_id,
    ))
}

#[wasm_bindgen]
pub fn unwrap_node_key(parent_key: &[u8], wrapped: &[u8], node_id: &str) -> R<Vec<u8>> {
    Ok(c::unwrap_node_key(&key(parent_key)?, wrapped, node_id)?
        .as_bytes()
        .to_vec())
}

/// `meta_json`: `{"name": "...", "mime": "...", "size": 0, "mtime": 0}`.
#[wasm_bindgen]
pub fn encrypt_metadata(node_key: &[u8], node_id: &str, meta_json: &str) -> R<Vec<u8>> {
    let meta: c::Metadata = serde_json::from_str(meta_json)?;
    Ok(c::encrypt_metadata(&key(node_key)?, node_id, &meta)?)
}

/// Returns the metadata as a JSON string.
#[wasm_bindgen]
pub fn decrypt_metadata(node_key: &[u8], node_id: &str, sealed: &[u8]) -> R<String> {
    let meta = c::decrypt_metadata(&key(node_key)?, node_id, sealed)?;
    Ok(serde_json::to_string(&meta)?)
}

#[wasm_bindgen]
pub fn wrap_content_key(
    node_key: &[u8],
    content_key: &[u8],
    node_id: &str,
    version_id: &str,
) -> R<Vec<u8>> {
    Ok(c::wrap_content_key(
        &key(node_key)?,
        &key(content_key)?,
        node_id,
        version_id,
    ))
}

#[wasm_bindgen]
pub fn unwrap_content_key(
    node_key: &[u8],
    wrapped: &[u8],
    node_id: &str,
    version_id: &str,
) -> R<Vec<u8>> {
    Ok(
        c::unwrap_content_key(&key(node_key)?, wrapped, node_id, version_id)?
            .as_bytes()
            .to_vec(),
    )
}

#[wasm_bindgen]
pub fn encrypt_chunk(
    content_key: &[u8],
    version_id: &str,
    index: u32,
    is_last: bool,
    plaintext: &[u8],
) -> R<Vec<u8>> {
    Ok(c::encrypt_chunk(
        &key(content_key)?,
        version_id,
        index,
        is_last,
        plaintext,
    ))
}

#[wasm_bindgen]
pub fn decrypt_chunk(
    content_key: &[u8],
    version_id: &str,
    index: u32,
    is_last: bool,
    sealed: &[u8],
) -> R<Vec<u8>> {
    Ok(c::decrypt_chunk(
        &key(content_key)?,
        version_id,
        index,
        is_last,
        sealed,
    )?)
}

#[wasm_bindgen]
pub fn seal_share_key(recipient_public: &[u8], node_key: &[u8], node_id: &str) -> R<Vec<u8>> {
    Ok(c::seal_share_key(
        recipient_public,
        &key(node_key)?,
        node_id,
    )?)
}

#[wasm_bindgen]
pub fn open_share_key(secret: &[u8], sealed: &[u8], node_id: &str) -> R<Vec<u8>> {
    Ok(c::open_share_key(&keypair(secret)?, sealed, node_id)?
        .as_bytes()
        .to_vec())
}
