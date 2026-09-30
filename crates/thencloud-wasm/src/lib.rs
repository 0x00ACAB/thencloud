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

/// Our secret key as JS holds it: the X25519 secret, followed by the
/// ML-KEM seed when the account has one.
fn keypair(secret: &[u8]) -> R<c::KeyPair> {
    let kp = c::KeyPair::from_secret(key(&secret[..secret.len().min(c::KEY_LEN)])?);
    Ok(match secret.len() {
        c::KEY_LEN => kp,
        n if n == c::KEY_LEN + c::PQ_SEED_LEN => {
            kp.with_pq(c::PqKeyPair::from_seed(&secret[c::KEY_LEN..])?)
        }
        _ => return Err(c::Error::KeyLength.into()),
    })
}

#[wasm_bindgen]
pub fn chunk_size() -> usize {
    c::CHUNK_SIZE
}

/// The size a file is padded to before encryption.
#[wasm_bindgen]
pub fn padded_size(size: f64) -> f64 {
    c::padded_size(size as u64) as f64
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
pub fn encrypt_private_data(mk: &[u8], user_id: &str, label: &str, plaintext: &[u8]) -> R<Vec<u8>> {
    Ok(c::encrypt_private_data(
        &key(mk)?,
        user_id,
        label,
        plaintext,
    ))
}

#[wasm_bindgen]
pub fn decrypt_private_data(mk: &[u8], user_id: &str, label: &str, sealed: &[u8]) -> R<Vec<u8>> {
    Ok(c::decrypt_private_data(&key(mk)?, user_id, label, sealed)?)
}

/// A recovery key as text for writing down ("ABCDE-FGHJK-...").
#[wasm_bindgen]
pub fn encode_recovery_key(k: &[u8]) -> R<String> {
    Ok(c::encode_recovery_key(&key(k)?))
}

/// Parse a typed recovery key; errors if it's malformed or has a typo.
#[wasm_bindgen]
pub fn decode_recovery_key(text: &str) -> R<Vec<u8>> {
    Ok(c::decode_recovery_key(text)?.as_bytes().to_vec())
}

#[wasm_bindgen]
pub fn derive_recovery_keys(k: &[u8]) -> R<AccountKeys> {
    let r = c::derive_recovery_keys(&key(k)?);
    Ok(AccountKeys {
        auth_key: r.auth_key.as_bytes().to_vec(),
        kek: r.kek.as_bytes().to_vec(),
    })
}

#[wasm_bindgen]
pub fn wrap_master_key_recovery(kek: &[u8], mk: &[u8]) -> R<Vec<u8>> {
    Ok(c::wrap_master_key_recovery(&key(kek)?, &key(mk)?))
}

#[wasm_bindgen]
pub fn unwrap_master_key_recovery(kek: &[u8], wrapped: &[u8]) -> R<Vec<u8>> {
    Ok(c::unwrap_master_key_recovery(&key(kek)?, wrapped)?
        .as_bytes()
        .to_vec())
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

/// A new ML-KEM-768 keypair: `secret` is the 64-byte seed.
#[wasm_bindgen]
pub fn generate_pq_keypair() -> KeyPair {
    let kp = c::PqKeyPair::generate();
    KeyPair {
        secret: kp.seed().to_vec(),
        public: kp.public.clone(),
    }
}

#[wasm_bindgen]
pub fn wrap_pq_private_key(mk: &[u8], seed: &[u8]) -> R<Vec<u8>> {
    Ok(c::wrap_pq_private_key(
        &key(mk)?,
        &c::PqKeyPair::from_seed(seed)?,
    ))
}

/// Returns the 64-byte seed.
#[wasm_bindgen]
pub fn unwrap_pq_private_key(mk: &[u8], wrapped: &[u8]) -> R<Vec<u8>> {
    Ok(c::unwrap_pq_private_key(&key(mk)?, wrapped)?
        .seed()
        .to_vec())
}

#[wasm_bindgen]
pub fn pq_public_key_from_seed(seed: &[u8]) -> R<Vec<u8>> {
    Ok(c::PqKeyPair::from_seed(seed)?.public.clone())
}

/// What a fingerprint covers: the X25519 key, plus the ML-KEM key's hash
/// (pass an empty array when there's none).
#[wasm_bindgen]
pub fn identity(x25519_public: &[u8], pq_public: &[u8]) -> Vec<u8> {
    c::identity(x25519_public, (!pq_public.is_empty()).then_some(pq_public))
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

#[wasm_bindgen]
pub fn seal_drop_key(
    owner_public: &[u8],
    node_key: &[u8],
    node_id: &str,
    folder_id: &str,
) -> R<Vec<u8>> {
    Ok(c::seal_drop_key(
        owner_public,
        &key(node_key)?,
        node_id,
        folder_id,
    )?)
}

#[wasm_bindgen]
pub fn open_drop_key(secret: &[u8], sealed: &[u8], node_id: &str, folder_id: &str) -> R<Vec<u8>> {
    Ok(
        c::open_drop_key(&keypair(secret)?, sealed, node_id, folder_id)?
            .as_bytes()
            .to_vec(),
    )
}

/// `record_json` is `ReportRecord` as JSON.
#[wasm_bindgen]
pub fn seal_report(
    admin_public: &[u8],
    record_json: &str,
    node_id: &str,
    version_id: &str,
) -> R<Vec<u8>> {
    let r: c::ReportRecord =
        serde_json::from_str(record_json).map_err(|e| c::Error::Metadata(e.to_string()))?;
    Ok(c::seal_report(admin_public, &r, node_id, version_id)?)
}

/// The record as JSON.
#[wasm_bindgen]
pub fn open_report(secret: &[u8], sealed: &[u8], node_id: &str, version_id: &str) -> R<String> {
    let r = c::open_report(&keypair(secret)?, sealed, node_id, version_id)?;
    Ok(serde_json::to_string(&r).map_err(|e| c::Error::Metadata(e.to_string()))?)
}

#[wasm_bindgen]
pub fn derive_app_password_keys(k: &[u8]) -> R<AccountKeys> {
    let r = c::derive_app_password_keys(&key(k)?);
    Ok(AccountKeys {
        auth_key: r.auth_key.as_bytes().to_vec(),
        kek: r.kek.as_bytes().to_vec(),
    })
}

#[wasm_bindgen]
pub fn wrap_master_key_app(kek: &[u8], mk: &[u8], app_password_id: &str) -> R<Vec<u8>> {
    Ok(c::wrap_master_key_app(
        &key(kek)?,
        &key(mk)?,
        app_password_id,
    ))
}

/// The input for the WebAuthn PRF extension.
#[wasm_bindgen]
pub fn passkey_prf_salt() -> Vec<u8> {
    c::passkey_prf_salt().to_vec()
}

#[wasm_bindgen]
pub fn wrap_master_key_passkey(prf_output: &[u8], mk: &[u8], credential_id: &[u8]) -> R<Vec<u8>> {
    let kek = c::derive_passkey_kek(prf_output)?;
    Ok(c::wrap_master_key_passkey(&kek, &key(mk)?, credential_id))
}

#[wasm_bindgen]
pub fn unwrap_master_key_passkey(
    prf_output: &[u8],
    wrapped: &[u8],
    credential_id: &[u8],
) -> R<Vec<u8>> {
    let kek = c::derive_passkey_kek(prf_output)?;
    Ok(c::unwrap_master_key_passkey(&kek, wrapped, credential_id)?
        .as_bytes()
        .to_vec())
}

#[wasm_bindgen]
pub fn encrypt_avatar(key_bytes: &[u8], owner: &str, image: &[u8]) -> R<Vec<u8>> {
    Ok(c::encrypt_avatar(&key(key_bytes)?, owner, image))
}

#[wasm_bindgen]
pub fn decrypt_avatar(key_bytes: &[u8], owner: &str, sealed: &[u8]) -> R<Vec<u8>> {
    Ok(c::decrypt_avatar(&key(key_bytes)?, owner, sealed)?)
}

/// A pronoun as it would be stored, or undefined if it can't be one.
#[wasm_bindgen]
pub fn clean_pronoun(p: &str) -> Option<String> {
    c::clean_pronoun(p)
}

/// `details_json` is `PersonDetails` as JSON.
#[wasm_bindgen]
pub fn encrypt_person_details(key_bytes: &[u8], owner: &str, details_json: &str) -> R<Vec<u8>> {
    let d: c::PersonDetails =
        serde_json::from_str(details_json).map_err(|e| JsError::new(&e.to_string()))?;
    Ok(c::encrypt_person_details(&key(key_bytes)?, owner, &d)?)
}

/// `PersonDetails` as JSON.
#[wasm_bindgen]
pub fn decrypt_person_details(key_bytes: &[u8], owner: &str, sealed: &[u8]) -> R<String> {
    let d = c::decrypt_person_details(&key(key_bytes)?, owner, sealed)?;
    serde_json::to_string(&d).map_err(|e| JsError::new(&e.to_string()))
}

/// The name as it would be stored, or undefined if it can't be one.
#[wasm_bindgen]
pub fn clean_display_name(name: &str) -> Option<String> {
    c::clean_display_name(name)
}

#[wasm_bindgen]
pub fn encrypt_display_name(key_bytes: &[u8], owner: &str, name: &str) -> R<Vec<u8>> {
    Ok(c::encrypt_display_name(&key(key_bytes)?, owner, name)?)
}

#[wasm_bindgen]
pub fn decrypt_display_name(key_bytes: &[u8], owner: &str, sealed: &[u8]) -> R<String> {
    Ok(c::decrypt_display_name(&key(key_bytes)?, owner, sealed)?)
}

#[wasm_bindgen]
pub fn seal_avatar_key(
    grantee_public: &[u8],
    key_bytes: &[u8],
    owner: &str,
    grantee: &str,
) -> R<Vec<u8>> {
    Ok(c::seal_avatar_key(
        grantee_public,
        &key(key_bytes)?,
        owner,
        grantee,
    )?)
}

#[wasm_bindgen]
pub fn open_avatar_key(secret: &[u8], sealed: &[u8], owner: &str, grantee: &str) -> R<Vec<u8>> {
    Ok(
        c::open_avatar_key(&keypair(secret)?, sealed, owner, grantee)?
            .as_bytes()
            .to_vec(),
    )
}

/// The folder-scoped tag the server uses to refuse duplicate names.
#[wasm_bindgen]
pub fn name_tag(folder_key: &[u8], name: &str) -> R<Vec<u8>> {
    Ok(c::name_tag(&key(folder_key)?, name))
}

/// The Argon2 salt for a link's password: run `derive_account_keys` with it
/// and the default parameters (in the KDF worker), then `derive_link_keys`.
#[wasm_bindgen]
pub fn link_password_salt(secret: &[u8]) -> Vec<u8> {
    c::link_password_salt(secret).to_vec()
}

/// A link's auth key and KEK, from the keys Argon2 made of its password.
#[wasm_bindgen]
pub fn derive_link_keys(secret: &[u8], auth_key: &[u8], kek: &[u8]) -> R<AccountKeys> {
    let from_password = c::AccountKeys {
        auth_key: key(auth_key)?,
        kek: key(kek)?,
    };
    let r = c::derive_link_keys(secret, &from_password);
    Ok(AccountKeys {
        auth_key: r.auth_key.as_bytes().to_vec(),
        kek: r.kek.as_bytes().to_vec(),
    })
}

#[wasm_bindgen]
pub fn wrap_link_key(kek: &[u8], node_key: &[u8], node_id: &str) -> R<Vec<u8>> {
    Ok(c::wrap_link_key(&key(kek)?, &key(node_key)?, node_id))
}

#[wasm_bindgen]
pub fn unwrap_link_key(kek: &[u8], wrapped: &[u8], node_id: &str) -> R<Vec<u8>> {
    Ok(c::unwrap_link_key(&key(kek)?, wrapped, node_id)?
        .as_bytes()
        .to_vec())
}

#[wasm_bindgen]
pub fn encrypt_link_secret(node_key: &[u8], secret: &[u8], node_id: &str) -> R<Vec<u8>> {
    Ok(c::encrypt_link_secret(
        &key(node_key)?,
        &key(secret)?,
        node_id,
    ))
}

#[wasm_bindgen]
pub fn decrypt_link_secret(node_key: &[u8], sealed: &[u8], node_id: &str) -> R<Vec<u8>> {
    Ok(c::decrypt_link_secret(&key(node_key)?, sealed, node_id)?
        .as_bytes()
        .to_vec())
}

#[wasm_bindgen]
pub fn encrypt_comment(
    node_key: &[u8],
    node_id: &str,
    comment_id: &str,
    author_id: &str,
    body: &[u8],
) -> R<Vec<u8>> {
    Ok(c::encrypt_comment(
        &key(node_key)?,
        node_id,
        comment_id,
        author_id,
        body,
    ))
}

#[wasm_bindgen]
pub fn decrypt_comment(
    node_key: &[u8],
    node_id: &str,
    comment_id: &str,
    author_id: &str,
    sealed: &[u8],
) -> R<Vec<u8>> {
    Ok(c::decrypt_comment(
        &key(node_key)?,
        node_id,
        comment_id,
        author_id,
        sealed,
    )?)
}

#[wasm_bindgen]
pub fn encrypt_thumbnail(
    node_key: &[u8],
    node_id: &str,
    version_id: &str,
    image: &[u8],
) -> R<Vec<u8>> {
    Ok(c::encrypt_thumbnail(
        &key(node_key)?,
        node_id,
        version_id,
        image,
    ))
}

#[wasm_bindgen]
pub fn decrypt_thumbnail(
    node_key: &[u8],
    node_id: &str,
    version_id: &str,
    sealed: &[u8],
) -> R<Vec<u8>> {
    Ok(c::decrypt_thumbnail(
        &key(node_key)?,
        node_id,
        version_id,
        sealed,
    )?)
}
