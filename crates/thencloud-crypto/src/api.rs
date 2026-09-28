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
    /// The ML-KEM-768 public key and its seed wrapped under the master key.
    #[serde(default)]
    pub pq_public_key: Option<B64>,
    #[serde(default)]
    pub enc_pq_private_key: Option<B64>,
    pub root: NewRootFolder,
    #[serde(default)]
    pub device_name: Option<String>,
    /// Invite token, needed when registration is invite-only.
    #[serde(default)]
    pub invite: Option<String>,
}

/// Who may create an account.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Registration {
    Open,
    Invite,
    Closed,
}

/// What the sign-in screen needs to know before anyone is signed in.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthOptions {
    pub registration: Registration,
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
    /// Absent for accounts made before post-quantum keys.
    #[serde(default)]
    pub pq_public_key: Option<B64>,
    #[serde(default)]
    pub enc_pq_private_key: Option<B64>,
}

/// Give an older account its post-quantum key. Only once.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SetPqKeyRequest {
    pub pq_public_key: B64,
    pub enc_pq_private_key: B64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Me {
    pub user_id: String,
    pub username: String,
    pub is_admin: bool,
    pub quota_bytes: i64,
    pub used_bytes: i64,
    pub keys: KeyBundle,
    /// Versions kept per file, including the current one.
    pub max_versions: i64,
    /// Days before trashed items are purged.
    pub trash_days: i64,
    /// When a recovery key was set up, if there is one.
    #[serde(default)]
    pub recovery_created_at: Option<i64>,
    /// When an authenticator app (TOTP) was set up, if there is one.
    #[serde(default)]
    pub totp_created_at: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionResponse {
    pub token: String,
    pub me: Me,
}

/// What signing in with a password returns: a session, or, when the account
/// has two-factor sign-in, a ticket to finish it with a second factor.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum LoginResponse {
    Session(Box<SessionResponse>),
    SecondFactor {
        second_factor: SecondFactorChallenge,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecondFactorChallenge {
    /// Pass back with the second factor. Valid for a few minutes.
    pub ticket: String,
    /// An authenticator app code is accepted.
    pub totp: bool,
    /// Set when the account has passkeys.
    #[serde(default)]
    pub passkey: Option<PasskeyRequest>,
}

/// What `navigator.credentials.get` needs.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PasskeyRequest {
    pub challenge: B64,
    /// The account's credentials (empty for a sign-in with a passkey alone).
    #[serde(default)]
    pub allow_credentials: Vec<B64>,
}

/// A WebAuthn assertion, as the browser returns it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PasskeyAssertion {
    pub credential_id: B64,
    pub client_data_json: B64,
    pub authenticator_data: B64,
    pub signature: B64,
    #[serde(default)]
    pub user_handle: Option<B64>,
}

/// Finish a password sign-in with a TOTP code or a passkey.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecondFactorRequest {
    pub ticket: String,
    #[serde(default)]
    pub totp_code: Option<String>,
    #[serde(default)]
    pub passkey: Option<PasskeyAssertion>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CurrentPassword {
    pub current_auth_key: B64,
}

/// A new TOTP secret, waiting to be confirmed with a code from the app.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TotpSetup {
    pub setup_id: String,
    /// Base32, as authenticator apps take it.
    pub secret: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnableTotpRequest {
    pub setup_id: String,
    pub code: String,
}

/// What `navigator.credentials.create` needs to make a passkey.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PasskeyCreationOptions {
    pub registration_id: String,
    pub challenge: B64,
    pub user_handle: B64,
    pub exclude_credentials: Vec<B64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegisterPasskeyRequest {
    pub registration_id: String,
    pub name: String,
    pub current_auth_key: B64,
    pub client_data_json: B64,
    pub attestation_object: B64,
    /// The master key wrapped under a key from the passkey's PRF output (see
    /// `wrap_master_key_passkey`), when the authenticator supports PRF.
    #[serde(default)]
    pub enc_master_key: Option<B64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Passkey {
    pub id: String,
    pub name: String,
    pub credential_id: B64,
    /// It can unlock the account on its own (PRF), not just confirm a sign-in.
    pub unlock: bool,
    pub created_at: i64,
    pub last_used_at: Option<i64>,
}

/// Sign in with a passkey alone. The challenge comes from
/// `/api/auth/passkey/options`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PasskeyLoginRequest {
    pub challenge: B64,
    pub assertion: PasskeyAssertion,
    #[serde(default)]
    pub device_name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PasskeyLoginResponse {
    pub token: String,
    pub me: Me,
    pub credential_id: B64,
    /// Unwrapped with the key from the passkey's PRF output.
    pub enc_master_key: B64,
}

/// A signed-in device, as listed in Settings. No IP addresses are kept.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceSession {
    pub id: String,
    pub device_name: String,
    pub created_at: i64,
    pub last_seen: i64,
    /// The session making this request.
    pub current: bool,
    /// Name of the app password it signed in with, if any.
    #[serde(default)]
    pub app_password: Option<String>,
}

// ---------------------------------------------------------------------------
// App passwords (per-device credentials for sync clients)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AppScope {
    /// Everything the account can do, except managing credentials.
    Full,
    /// Reading and downloading only.
    Read,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateAppPasswordRequest {
    /// Client-chosen id, bound into `enc_master_key`.
    pub id: String,
    pub name: String,
    pub scope: AppScope,
    /// Proves the account password.
    pub current_auth_key: B64,
    /// The auth half of the app password (see `derive_app_password_keys`).
    pub auth_key: B64,
    /// The master key wrapped under the app password's KEK.
    pub enc_master_key: B64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppPassword {
    pub id: String,
    pub name: String,
    pub scope: AppScope,
    pub created_at: i64,
    pub last_used_at: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppLoginRequest {
    pub auth_key: B64,
    #[serde(default)]
    pub device_name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppLoginResponse {
    pub token: String,
    pub me: Me,
    pub app_password_id: String,
    pub scope: AppScope,
    pub enc_master_key: B64,
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
    /// Whether this version has an encrypted thumbnail (`encrypt_thumbnail`).
    #[serde(default)]
    pub has_thumbnail: bool,
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
    /// Whether the node has a name tag (older ones may not).
    #[serde(default)]
    pub name_tagged: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateFolderRequest {
    pub id: String,
    pub parent_id: String,
    pub enc_key: B64,
    pub enc_metadata: B64,
    /// `name_tag(parent key, name)`: lets the server refuse a duplicate name.
    #[serde(default)]
    pub name_tag: Option<B64>,
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
    /// The name tag for the new name or parent. Renaming or moving without
    /// one leaves the node untagged.
    #[serde(default)]
    pub name_tag: Option<B64>,
}

/// Name tags for existing children that don't have one yet.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NameTags {
    pub tags: Vec<NameTagEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NameTagEntry {
    pub id: String,
    pub name_tag: B64,
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
// Versions
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileVersion {
    pub id: String,
    pub enc_content_key: B64,
    /// Metadata the version was uploaded with (plaintext size, mtime).
    pub enc_metadata: B64,
    pub chunk_count: u32,
    /// Stored (ciphertext) size in bytes.
    pub size: i64,
    pub created_at: i64,
    pub created_by: String,
    pub current: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RestoreVersionRequest {
    /// The node's metadata re-encrypted for the restored version (current
    /// name, the version's size and mtime).
    pub enc_metadata: B64,
    #[serde(default)]
    pub if_revision: Option<i64>,
}

// ---------------------------------------------------------------------------
// Trash
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrashItem {
    pub node: Node,
    /// Chain from the owner's root folder down to (and including) the
    /// trashed node, so the client can unwrap its key and show where it was.
    pub path: Vec<Node>,
    pub trashed_at: i64,
    pub trashed_by: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RestoreTrashRequest {
    /// Restore into a different folder (e.g. when the original one is gone).
    /// Requires `enc_key` re-wrapped under that folder's key.
    #[serde(default)]
    pub parent_id: Option<String>,
    #[serde(default)]
    pub enc_key: Option<B64>,
    /// A new name (when the old one is taken) and its tag.
    #[serde(default)]
    pub enc_metadata: Option<B64>,
    #[serde(default)]
    pub name_tag: Option<B64>,
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
    /// For a new file: its name tag (see `name_tag`).
    #[serde(default)]
    pub name_tag: Option<B64>,
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
    /// Their ML-KEM-768 key, if they have one: seal to both.
    #[serde(default)]
    pub pq_public_key: Option<B64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateShareRequest {
    pub node_id: String,
    pub recipient: String,
    /// Node key sealed to the recipient's public key.
    pub wrapped_key: B64,
    pub permission: Permission,
    /// Unix seconds; the share ends then.
    #[serde(default)]
    pub expires_at: Option<i64>,
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
    #[serde(default)]
    pub owner_pq_public_key: Option<B64>,
    pub permission: Permission,
    pub wrapped_key: B64,
    pub node: Node,
    pub created_at: i64,
    #[serde(default)]
    pub expires_at: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutgoingShare {
    pub id: String,
    pub recipient: String,
    pub permission: Permission,
    pub node_id: String,
    pub created_at: i64,
    #[serde(default)]
    pub expires_at: Option<i64>,
}

// ---------------------------------------------------------------------------
// Public links
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateLinkRequest {
    pub node_id: String,
    /// For a link with a password: the auth key from
    /// `derive_link_password_keys`. The password itself never reaches the
    /// server.
    #[serde(default)]
    pub password_auth: Option<B64>,
    /// With a password (except upload-only links): the node key wrapped
    /// under the link's KEK (`wrap_link_key`), handed to visitors who give
    /// the password, and the link's secret under the node key
    /// (`encrypt_link_secret`) so the owner can show the link again.
    #[serde(default)]
    pub enc_link_key: Option<B64>,
    #[serde(default)]
    pub enc_link_secret: Option<B64>,
    /// Unix seconds.
    #[serde(default)]
    pub expires_at: Option<i64>,
    /// A file drop: visitors can add files to the folder but see nothing
    /// in it. The link carries the owner's public key after `#` instead of
    /// the folder key.
    #[serde(default)]
    pub upload_only: bool,
    /// Stop working after this many opens (1: after the first). An open is
    /// a visit to the link page; the visit itself can then fetch everything.
    #[serde(default)]
    pub max_opens: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Link {
    pub id: String,
    pub token: String,
    pub node_id: String,
    pub has_password: bool,
    pub expires_at: Option<i64>,
    pub created_at: i64,
    #[serde(default)]
    pub upload_only: bool,
    #[serde(default)]
    pub max_opens: Option<i64>,
    #[serde(default)]
    pub opens: i64,
    /// For a link with a password: its secret, sealed under the node key.
    #[serde(default)]
    pub enc_link_secret: Option<B64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnlockLinkRequest {
    /// The auth key from `derive_link_password_keys`, not the password.
    pub auth: B64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnlockLinkResponse {
    /// Pass back in the `X-Link-Token` header.
    pub link_token: String,
    pub expires_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PublicLinkInfo {
    /// Absent for upload-only links.
    pub node: Option<Node>,
    pub expires_at: Option<i64>,
    #[serde(default)]
    pub upload_only: bool,
    /// Who files dropped through an upload-only link go to, and the folder
    /// id to bind into their sealed keys.
    #[serde(default)]
    pub owner: Option<String>,
    #[serde(default)]
    pub folder_id: Option<String>,
    /// The owner's ML-KEM key, for upload-only links whose `#` part names
    /// its hash. Checked against it, so the server can't swap it.
    #[serde(default)]
    pub owner_pq_public_key: Option<B64>,
    /// For a link with a limited number of opens: this visit's token, to
    /// pass back in the `X-Link-Token` header.
    #[serde(default)]
    pub link_token: Option<String>,
    /// For a link with a password, once it's given: the node key wrapped
    /// under the link's KEK, which needs the password and the secret after
    /// `#` to open.
    #[serde(default)]
    pub enc_link_key: Option<B64>,
}

/// A file added through an upload-only link, waiting for the folder owner
/// to open its sealed key and wrap it under the folder key.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DroppedFile {
    pub node: Node,
    /// The node key sealed to the owner's public key (see `seal_drop_key`).
    pub sealed_key: B64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdoptDropRequest {
    /// The node key wrapped under the folder's key.
    pub enc_key: B64,
    /// The metadata under the new key (with a new name, if the dropped one
    /// is taken), and the name tag.
    #[serde(default)]
    pub enc_metadata: Option<B64>,
    #[serde(default)]
    pub name_tag: Option<B64>,
    /// The file's content key re-wrapped under the new node key, so the
    /// key the visitor chose stops mattering. Requires `enc_metadata`.
    #[serde(default)]
    pub enc_content_key: Option<B64>,
}

/// Optional body for finishing an upload of a new file: a new name (and
/// its tag) if the one it started with was taken meanwhile.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct FinishUploadRequest {
    #[serde(default)]
    pub enc_metadata: Option<B64>,
    #[serde(default)]
    pub name_tag: Option<B64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErrorBody {
    pub error: String,
    pub message: String,
}

// ---------------------------------------------------------------------------
// Administration
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdminUser {
    pub id: String,
    pub username: String,
    pub is_admin: bool,
    pub disabled: bool,
    pub quota_bytes: i64,
    pub used_bytes: i64,
    pub created_at: i64,
    /// Most recent activity of any of their sessions.
    pub last_seen: Option<i64>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct UpdateUserRequest {
    #[serde(default)]
    pub quota_bytes: Option<i64>,
    #[serde(default)]
    pub disabled: Option<bool>,
    #[serde(default)]
    pub is_admin: Option<bool>,
}

/// Who may use the video downloader, the one tool where the server sees
/// what it handles (see MILESTONES.md). Off by default.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum DownloaderAccess {
    #[default]
    Off,
    Admins,
    Everyone,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdminSettings {
    pub registration: Registration,
    /// Quota for new accounts (from the server's configuration).
    pub default_quota: i64,
    pub downloader: DownloaderAccess,
    /// yt-dlp's version, or None if it isn't installed on the server.
    pub yt_dlp_version: Option<String>,
    /// Whether ffmpeg is installed, so video and audio can be merged
    /// (needed for most YouTube videos).
    pub downloader_can_merge: bool,
    /// Largest download passed through, in bytes.
    pub downloader_max_bytes: u64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct UpdateSettingsRequest {
    #[serde(default)]
    pub registration: Option<Registration>,
    #[serde(default)]
    pub downloader: Option<DownloaderAccess>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Invite {
    pub id: String,
    pub created_by: String,
    pub created_at: i64,
    pub expires_at: i64,
    pub used_by: Option<String>,
    pub used_at: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateInviteRequest {
    /// How long the invite stays valid, in days (1 to 90).
    pub days: i64,
}

/// A new invite. `token` is shown once; the server keeps only its hash.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreatedInvite {
    pub invite: Invite,
    pub token: String,
}

/// Counts only; nothing about what is stored.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerStats {
    pub users: i64,
    pub disabled_users: i64,
    pub active_sessions: i64,
    pub used_bytes: i64,
    pub quota_bytes: i64,
    pub files: i64,
    pub folders: i64,
    pub versions: i64,
    pub shares: i64,
    pub public_links: i64,
}

// ---------------------------------------------------------------------------
// Recovery keys
// ---------------------------------------------------------------------------

/// Set up (or replace) a recovery key. Needs the current password.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SetRecoveryRequest {
    pub current_auth_key: B64,
    pub recovery_auth_key: B64,
    pub enc_master_key_recovery: B64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RemoveRecoveryRequest {
    pub current_auth_key: B64,
}

/// Delete your own account, proving the password again.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeleteAccountRequest {
    pub current_auth_key: B64,
}

/// Step one of a password reset: prove the recovery key, get the master key
/// wrapped under it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecoveryUnlockRequest {
    pub username: String,
    pub recovery_auth_key: B64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecoveryUnlockResponse {
    pub enc_master_key_recovery: B64,
}

/// Step two: a new password, with the master key re-wrapped under it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecoveryResetRequest {
    pub username: String,
    pub recovery_auth_key: B64,
    pub new_auth_key: B64,
    pub new_kdf_salt: B64,
    pub new_kdf_params: KdfParams,
    pub new_enc_master_key: B64,
    #[serde(default)]
    pub device_name: Option<String>,
}

// ---------------------------------------------------------------------------
// Private account data (verified contacts), encrypted under the master key
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrivateData {
    /// None until the client first saves something.
    pub data: Option<B64>,
    pub revision: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PutPrivateData {
    pub data: B64,
    /// The revision this change is based on; 409 if it moved on.
    pub if_revision: i64,
}

/// Unsaved edits to a text file, encrypted under the editor's master key
/// (see `encrypt_private_data`, labelled with the node id).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Draft {
    pub data: B64,
    /// The file's revision the draft started from.
    pub base_revision: i64,
    #[serde(default)]
    pub updated_at: i64,
}

// ---------------------------------------------------------------------------
// Comments (encrypted under the node key; see `encrypt_comment`)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Comment {
    pub id: String,
    pub node_id: String,
    pub author_id: String,
    /// The author's username. Comments go when their author's account does.
    pub author: String,
    /// To the hour; the exact time is inside the encrypted body.
    pub created_at: i64,
    pub enc_body: B64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateCommentRequest {
    /// Chosen by the client: it's bound into `enc_body`.
    pub id: String,
    pub enc_body: B64,
}

// ---------------------------------------------------------------------------
// Profile pictures (encrypted; see `encrypt_avatar`)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SetAvatar {
    /// The picture, encrypted under the avatar key.
    pub data: B64,
    /// The avatar key, encrypted under the master key.
    pub enc_key: B64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct MyAvatar {
    pub data: Option<B64>,
    pub enc_key: Option<B64>,
    /// Who has been given the avatar key.
    pub grantees: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AvatarGrant {
    /// The avatar key sealed to the grantee (see `seal_avatar_key`).
    pub sealed_key: B64,
}

/// Someone else's picture, for a user they gave their avatar key to.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserAvatar {
    pub data: B64,
    pub sealed_key: B64,
    pub updated_at: i64,
}

// ---------------------------------------------------------------------------
// Tools
// ---------------------------------------------------------------------------

/// Which tools this user can use.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolsInfo {
    pub video_downloader: bool,
    pub downloader_max_bytes: u64,
    /// Whether separate video and audio can be merged (up to 1080p), or only
    /// formats offered as one file work.
    pub downloader_can_merge: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum VideoKind {
    Video,
    Audio,
}

/// Highest resolution to fetch. Up to 1080p, H.264 is preferred (it plays
/// everywhere); `Best` takes the tallest there is, whatever the codec.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum VideoQuality {
    #[serde(rename = "480")]
    P480,
    #[serde(rename = "720")]
    P720,
    #[serde(rename = "1080")]
    P1080,
    #[serde(rename = "best")]
    Best,
}

impl VideoQuality {
    pub const ALL: [VideoQuality; 4] = [Self::P480, Self::P720, Self::P1080, Self::Best];

    pub fn max_height(self) -> Option<u64> {
        match self {
            Self::P480 => Some(480),
            Self::P720 => Some(720),
            Self::P1080 => Some(1080),
            Self::Best => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VideoLinkRequest {
    pub url: String,
    #[serde(default)]
    pub kind: Option<VideoKind>,
    /// Defaults to 1080p.
    #[serde(default)]
    pub quality: Option<VideoQuality>,
}

/// One video in a playlist.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlaylistEntry {
    pub url: String,
    pub title: String,
    pub duration: Option<f64>,
}

/// What a link points at, and what the downloader would fetch for it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VideoInfo {
    pub title: String,
    pub site: String,
    pub uploader: Option<String>,
    /// Seconds.
    pub duration: Option<f64>,
    /// File extension and approximate size of the video (with sound) and
    /// audio-only downloads, when the site offers them as one file.
    pub video: Option<VideoOption>,
    pub audio: Option<VideoOption>,
    /// The video at each quality that gives a different result, lowest
    /// first.
    #[serde(default)]
    pub qualities: Vec<VideoOption>,
    /// For a playlist link: its videos (and no `video` or `audio`).
    #[serde(default)]
    pub entries: Vec<PlaylistEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VideoOption {
    pub ext: String,
    pub size: Option<u64>,
    /// Video height in pixels, for video.
    pub height: Option<u32>,
    /// The quality to ask for to get this, for video.
    #[serde(default)]
    pub quality: Option<VideoQuality>,
}
