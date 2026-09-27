//! JSON wire types shared by the server and native clients.
//!
//! Binary fields are carried as unpadded base64url strings ([`B64`]).
//! Nothing in here is ever plaintext user data: names, sizes as the user
//! sees them, and keys only cross the wire in encrypted form.

use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::KdfParams;

/// Bytes serialised as unpadded base64url.
#[derive(Clone, Default, PartialEq, Eq)]
pub struct B64(pub Vec<u8>);

impl std::fmt::Debug for B64 {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "B64({} bytes)", self.0.len())
    }
}

impl From<Vec<u8>> for B64 {
    fn from(v: Vec<u8>) -> Self {
        B64(v)
    }
}

impl std::ops::Deref for B64 {
    type Target = [u8];
    fn deref(&self) -> &[u8] {
        &self.0
    }
}

impl Serialize for B64 {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&crate::b64_encode(&self.0))
    }
}

impl<'de> Deserialize<'de> for B64 {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        crate::b64_decode(&s)
            .map(B64)
            .map_err(serde::de::Error::custom)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, PartialOrd, Ord)]
#[serde(rename_all = "lowercase")]
pub enum Permission {
    Read,
    Write,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum NodeKind {
    File,
    Folder,
}

// ---------------------------------------------------------------------------
// Auth
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PreloginRequest {
    pub username: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PreloginResponse {
    pub kdf_salt: B64,
    pub kdf_params: KdfParams,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NewRootFolder {
    pub id: String,
    /// Root folder key wrapped under the master key.
    pub enc_key: B64,
    pub enc_metadata: B64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegisterRequest {
    pub username: String,
    pub auth_key: B64,
    pub kdf_salt: B64,
    pub kdf_params: KdfParams,
    pub enc_master_key: B64,
    pub public_key: B64,
    pub enc_private_key: B64,
    pub root: NewRootFolder,
    #[serde(default)]
    pub device_name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoginRequest {
    pub username: String,
    pub auth_key: B64,
    #[serde(default)]
    pub device_name: Option<String>,
}

/// Encrypted key material the client needs after login.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeyBundle {
    pub kdf_salt: B64,
    pub kdf_params: KdfParams,
    pub enc_master_key: B64,
    pub public_key: B64,
    pub enc_private_key: B64,
    pub root_node_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Me {
    pub user_id: String,
    pub username: String,
    pub is_admin: bool,
    pub quota_bytes: i64,
    pub used_bytes: i64,
    pub keys: KeyBundle,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionResponse {
    pub token: String,
    pub me: Me,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChangePasswordRequest {
    pub current_auth_key: B64,
    pub new_auth_key: B64,
    pub new_kdf_salt: B64,
    pub new_kdf_params: KdfParams,
    pub new_enc_master_key: B64,
}

// ---------------------------------------------------------------------------
// Nodes
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VersionInfo {
    pub id: String,
    pub enc_content_key: B64,
    pub chunk_count: u32,
    /// Stored (ciphertext) size in bytes.
    pub size: i64,
    pub created_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Node {
    pub id: String,
    pub parent_id: Option<String>,
    pub kind: NodeKind,
    pub owner: String,
    /// This node's key, wrapped under the parent's key (or the master key
    /// for a root folder).
    pub enc_key: B64,
    pub enc_metadata: B64,
    pub revision: i64,
    pub created_at: i64,
    pub updated_at: i64,
    pub version: Option<VersionInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateFolderRequest {
    pub id: String,
    pub parent_id: String,
    pub enc_key: B64,
    pub enc_metadata: B64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct UpdateNodeRequest {
    /// Re-encrypted metadata (rename).
    #[serde(default)]
    pub enc_metadata: Option<B64>,
    /// New parent (move). Requires `enc_key` re-wrapped under the new
    /// parent's key.
    #[serde(default)]
    pub parent_id: Option<String>,
    #[serde(default)]
    pub enc_key: Option<B64>,
    /// Optimistic concurrency: fail with 409 if the node changed.
    #[serde(default)]
    pub if_revision: Option<i64>,
}

/// The chain of nodes from the top-most node the caller can access down to
/// the requested node. For the owner this starts at their root folder. For
/// a share recipient it starts at the shared node, and `share` carries the
/// key wrapped for them.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodePath {
    pub nodes: Vec<Node>,
    pub share: Option<ShareKey>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShareKey {
    pub share_id: String,
    pub wrapped_key: B64,
    pub permission: Permission,
}

// ---------------------------------------------------------------------------
// Uploads
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateUploadRequest {
    /// Client-chosen id of the new node, or the id of an existing file
    /// node when uploading a new version.
    pub node_id: String,
    /// Set for a new file: the folder to create it in.
    #[serde(default)]
    pub parent_id: Option<String>,
    /// Required for a new file.
    #[serde(default)]
    pub enc_key: Option<B64>,
    pub enc_metadata: B64,
    pub version_id: String,
    pub enc_content_key: B64,
    pub chunk_count: u32,
    /// For new versions: fail with 409 if the node changed meanwhile.
    #[serde(default)]
    pub if_revision: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UploadResponse {
    pub upload_id: String,
    pub max_chunk_bytes: usize,
    pub expires_at: i64,
}

// ---------------------------------------------------------------------------
// Shares
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserPublicKey {
    pub username: String,
    pub public_key: B64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateShareRequest {
    pub node_id: String,
    pub recipient: String,
    /// Node key sealed to the recipient's public key.
    pub wrapped_key: B64,
    pub permission: Permission,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateShareRequest {
    pub permission: Permission,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IncomingShare {
    pub id: String,
    pub owner: String,
    pub owner_public_key: B64,
    pub permission: Permission,
    pub wrapped_key: B64,
    pub node: Node,
    pub created_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutgoingShare {
    pub id: String,
    pub recipient: String,
    pub permission: Permission,
    pub node_id: String,
    pub created_at: i64,
}

// ---------------------------------------------------------------------------
// Public links
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateLinkRequest {
    pub node_id: String,
    /// Optional access password. This is a server-side gate only; the
    /// decryption key lives in the URL fragment and never reaches the
    /// server.
    #[serde(default)]
    pub password: Option<String>,
    /// Unix seconds.
    #[serde(default)]
    pub expires_at: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Link {
    pub id: String,
    pub token: String,
    pub node_id: String,
    pub has_password: bool,
    pub expires_at: Option<i64>,
    pub created_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnlockLinkRequest {
    pub password: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnlockLinkResponse {
    /// Pass back in the `X-Link-Token` header.
    pub link_token: String,
    pub expires_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PublicLinkInfo {
    pub node: Node,
    pub expires_at: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErrorBody {
    pub error: String,
    pub message: String,
}
