//! A thencloud client for the command line. Like the web client, it does
//! all the cryptography locally with `thencloud-crypto`: the server only
//! ever gets ciphertext, wrapped keys and name tags. It signs in with an
//! app password (made in Settings), never the account password.

use std::fs;
use std::io::{self, Read, Write};
use std::path::Path;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

pub mod backup;
#[cfg(target_os = "linux")]
pub mod mount;
pub mod nextcloud;
pub mod serve;
pub mod verify;

use serde::Serialize;
use serde::de::DeserializeOwned;
use thencloud_crypto::api::*;
use thencloud_crypto::{self as c, CHUNK_SIZE, Key, Metadata};

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug)]
pub enum Error {
    /// The server said no: HTTP status, error code and message.
    Api(u16, String, String),
    Http(String),
    Crypto(c::Error),
    Io(io::Error),
    Usage(String),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Api(status, _, msg) => write!(f, "{msg} (HTTP {status})"),
            Error::Http(e) => write!(f, "can't reach the server: {e}"),
            Error::Crypto(e) => write!(f, "decryption failed: {e}"),
            Error::Io(e) => write!(f, "{e}"),
            Error::Usage(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for Error {}

impl From<c::Error> for Error {
    fn from(e: c::Error) -> Self {
        Error::Crypto(e)
    }
}

impl From<io::Error> for Error {
    fn from(e: io::Error) -> Self {
        Error::Io(e)
    }
}

impl From<ureq::Error> for Error {
    fn from(e: ureq::Error) -> Self {
        Error::Http(e.to_string())
    }
}

/// A file or folder with its decrypted key and metadata.
#[derive(Clone)]
pub struct Entry {
    pub node: Node,
    pub key: Key,
    pub meta: Metadata,
}

impl Entry {
    pub fn is_folder(&self) -> bool {
        matches!(self.node.kind, NodeKind::Folder)
    }
}

pub struct Client {
    base: String,
    agent: ureq::Agent,
    token: String,
    mk: Key,
    pub me: Me,
    /// Read-only app passwords can only list and download.
    pub scope: AppScope,
}

/// Parse an app password as shown in Settings ("ABCDE-FGHJK-...").
pub fn parse_app_password(text: &str) -> Result<Key> {
    c::decode_recovery_key(text).map_err(|_| {
        Error::Usage("that doesn't look like an app password; check it for typos".into())
    })
}

fn agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .http_status_as_error(false)
        .timeout_global(Some(Duration::from_secs(300)))
        .build()
        .into()
}

fn check(mut res: ureq::http::Response<ureq::Body>) -> Result<ureq::http::Response<ureq::Body>> {
    let status = res.status().as_u16();
    if status < 400 {
        return Ok(res);
    }
    let body: Option<ErrorBody> = res.body_mut().read_json().ok();
    Err(match body {
        Some(b) => Error::Api(status, b.error, b.message),
        None => Error::Api(
            status,
            String::new(),
            format!("request failed with HTTP {status}"),
        ),
    })
}

impl Client {
    /// Sign in with an app password.
    pub fn login(server: &str, app_password: &Key, device: &str) -> Result<Client> {
        let base = server.trim_end_matches('/').to_string();
        let agent = agent();
        let keys = c::derive_app_password_keys(app_password);
        let res = agent
            .post(format!("{base}/api/auth/app-login"))
            .send_json(&AppLoginRequest {
                auth_key: B64(keys.auth_key.as_bytes().to_vec()),
                device_name: Some(device.into()),
            })?;
        let r: AppLoginResponse = check(res)?.body_mut().read_json()?;
        let mk = c::unwrap_master_key_app(&keys.kek, &r.enc_master_key, &r.app_password_id)?;
        let kp = c::unwrap_private_key(&mk, &r.me.keys.enc_private_key)?;
        if kp.public.as_slice() != r.me.keys.public_key.as_ref() {
            return Err(Error::Usage(
                "the public key on the server doesn't match your private key; refusing to continue"
                    .into(),
            ));
        }
        Ok(Client {
            base,
            agent,
            token: r.token,
            mk,
            me: r.me,
            scope: r.scope,
        })
    }

    pub fn logout(&self) -> Result<()> {
        self.send::<()>("POST", "/api/auth/logout", None)
            .map(|_| ())
    }

    fn url(&self, path: &str) -> String {
        format!("{}{path}", self.base)
    }

    fn auth(&self) -> String {
        format!("Bearer {}", self.token)
    }

    fn send<B: Serialize>(
        &self,
        method: &str,
        path: &str,
        body: Option<&B>,
    ) -> Result<ureq::http::Response<ureq::Body>> {
        let url = self.url(path);
        let auth = self.auth();
        let res = match (method, body) {
            ("GET", _) => self.agent.get(url).header("Authorization", auth).call()?,
            ("DELETE", _) => self
                .agent
                .delete(url)
                .header("Authorization", auth)
                .call()?,
            ("POST", Some(b)) => self
                .agent
                .post(url)
                .header("Authorization", auth)
                .send_json(b)?,
            ("POST", None) => self
                .agent
                .post(url)
                .header("Authorization", auth)
                .send_empty()?,
            ("PATCH", Some(b)) => self
                .agent
                .patch(url)
                .header("Authorization", auth)
                .send_json(b)?,
            _ => return Err(Error::Usage(format!("unsupported request {method} {path}"))),
        };
        check(res)
    }

    fn get_json<T: DeserializeOwned>(&self, path: &str) -> Result<T> {
        Ok(self.send::<()>("GET", path, None)?.body_mut().read_json()?)
    }

    fn post_json<B: Serialize, T: DeserializeOwned>(&self, path: &str, body: &B) -> Result<T> {
        Ok(self
            .send("POST", path, Some(body))?
            .body_mut()
            .read_json()?)
    }

    /// The root folder, "My files".
    pub fn root(&self) -> Result<Entry> {
        let id = self.me.keys.root_node_id.clone();
        let node: Node = self.get_json(&format!("/api/nodes/{id}"))?;
        let key = c::unwrap_node_key(&self.mk, &node.enc_key, &id)?;
        let meta = c::decrypt_metadata(&key, &id, &node.enc_metadata)?;
        Ok(Entry { node, key, meta })
    }

    /// One page of the change feed: node ids that changed in our tree and
    /// under anything shared with us after `since`. Without `since`, just
    /// the current cursor (take it before walking the tree).
    pub fn changes(&self, since: Option<i64>) -> Result<ChangeFeed> {
        match since {
            Some(s) => self.get_json(&format!("/api/changes?since={s}")),
            None => self.get_json("/api/changes"),
        }
    }

    /// The folder a node is in now, or `None` if it's gone (or no longer
    /// ours to see).
    pub fn parent_of(&self, id: &str) -> Result<Option<String>> {
        match self.get_json::<Node>(&format!("/api/nodes/{id}")) {
            Ok(n) => Ok(n.parent_id),
            Err(Error::Api(404, ..)) => Ok(None),
            Err(e) => Err(e),
        }
    }

    /// A folder's children, folders first, then by name.
    pub fn list(&self, folder: &Entry) -> Result<Vec<Entry>> {
        let mut nodes = Vec::new();
        let mut after: Option<String> = None;
        loop {
            let mut path = format!("/api/nodes/{}/children?limit=2000", folder.node.id);
            if let Some(a) = &after {
                path.push_str(&format!("&after={}", url_escape(a)));
            }
            let page: NodePage = self.get_json(&path)?;
            nodes.extend(page.nodes);
            match page.next {
                Some(n) => after = Some(n),
                None => break,
            }
        }
        let mut out = Vec::with_capacity(nodes.len());
        for node in nodes {
            let key = c::unwrap_node_key(&folder.key, &node.enc_key, &node.id)?;
            let meta = c::decrypt_metadata(&key, &node.id, &node.enc_metadata)?;
            out.push(Entry { node, key, meta });
        }
        out.sort_by(|a, b| {
            b.is_folder()
                .cmp(&a.is_folder())
                .then_with(|| a.meta.name.to_lowercase().cmp(&b.meta.name.to_lowercase()))
        });
        Ok(out)
    }

    /// Find `path` ("Photos/2024/a.jpg") under the root. Names are matched
    /// after decryption, exactly first, then ignoring case.
    pub fn resolve(&self, path: &str) -> Result<Entry> {
        let mut here = self.root()?;
        for part in path.split('/').filter(|p| !p.is_empty() && *p != ".") {
            if !here.is_folder() {
                return Err(Error::Usage(format!("{} is not a folder", here.meta.name)));
            }
            let kids = self.list(&here)?;
            here = kids
                .iter()
                .find(|k| k.meta.name == part)
                .or_else(|| kids.iter().find(|k| k.meta.name.eq_ignore_ascii_case(part)))
                .cloned()
                .ok_or_else(|| Error::Usage(format!("no such file or folder: {part}")))?;
        }
        Ok(here)
    }

    pub fn mkdir(&self, parent: &Entry, name: &str) -> Result<Entry> {
        let id = c::new_id();
        let key = Key::generate();
        let meta = Metadata {
            name: name.into(),
            mime: None,
            size: 0,
            mtime: now_ms(),
            changed: Some(now_ms()),
            taken: None,
        };
        let req = CreateFolderRequest {
            id: id.clone(),
            parent_id: parent.node.id.clone(),
            enc_key: B64(c::wrap_node_key(&parent.key, &key, &id)),
            enc_metadata: B64(c::encrypt_metadata(&key, &id, &meta)?),
            name_tag: Some(B64(c::name_tag(&parent.key, name))),
        };
        let node: Node = self.post_json("/api/nodes/folder", &req)?;
        Ok(Entry { node, key, meta })
    }

    /// Rename, move (to `parent`) or change the mtime: `meta` is the node's
    /// new metadata. Fails with 409 if someone else changed it meanwhile.
    pub fn update(&self, e: &Entry, parent: &Entry, mut meta: Metadata) -> Result<Entry> {
        meta.changed = Some(now_ms());
        let moving = e.node.parent_id.as_deref() != Some(parent.node.id.as_str());
        let req = UpdateNodeRequest {
            enc_metadata: Some(B64(c::encrypt_metadata(&e.key, &e.node.id, &meta)?)),
            parent_id: moving.then(|| parent.node.id.clone()),
            enc_key: moving.then(|| B64(c::wrap_node_key(&parent.key, &e.key, &e.node.id))),
            if_revision: Some(e.node.revision),
            name_tag: Some(B64(c::name_tag(&parent.key, &meta.name))),
        };
        let node: Node = self
            .send("PATCH", &format!("/api/nodes/{}", e.node.id), Some(&req))?
            .body_mut()
            .read_json()?;
        Ok(Entry {
            node,
            key: e.key.clone(),
            meta,
        })
    }

    /// Move to the trash.
    pub fn trash(&self, e: &Entry) -> Result<()> {
        self.send::<()>("DELETE", &format!("/api/nodes/{}", e.node.id), None)
            .map(|_| ())
    }

    /// Delete something already in the trash for good.
    pub fn purge(&self, id: &str) -> Result<()> {
        self.send::<()>("DELETE", &format!("/api/trash/{id}"), None)
            .map(|_| ())
    }

    /// The account as it is now (for the quota).
    pub fn fetch_me(&self) -> Result<Me> {
        self.get_json("/api/me")
    }

    /// Fetch and decrypt chunk `index` of a file's current version, padding
    /// included: every chunk but the last is exactly `CHUNK_SIZE` bytes.
    pub fn chunk(&self, file: &Entry, index: u32) -> Result<Vec<u8>> {
        let v = file
            .node
            .version
            .as_ref()
            .ok_or_else(|| Error::Usage(format!("{} has no content", file.meta.name)))?;
        let ck = c::unwrap_content_key(&file.key, &v.enc_content_key, &file.node.id, &v.id)?;
        let mut res = self.send::<()>(
            "GET",
            &format!("/api/nodes/{}/chunks/{index}", file.node.id),
            None,
        )?;
        let enc = res
            .body_mut()
            .with_config()
            .limit(c::MAX_ENCRYPTED_CHUNK as u64 + 1024)
            .read_to_vec()?;
        let last = index + 1 == v.chunk_count;
        let plain = c::decrypt_chunk(&ck, &v.id, index, last, &enc)?;
        let end = index as u64 * CHUNK_SIZE as u64 + plain.len() as u64;
        if (!last && plain.len() != CHUNK_SIZE) || (last && end < file.meta.size) {
            return Err(Error::Usage(
                "the decrypted size doesn't match the file's metadata".into(),
            ));
        }
        Ok(plain)
    }

    /// Download and decrypt a file's current version into `out`, piece by
    /// piece. Returns the number of bytes written.
    pub fn download(&self, file: &Entry, out: &mut impl Write) -> Result<u64> {
        let count = file.node.version.as_ref().map_or(0, |v| v.chunk_count);
        let size = file.meta.size;
        let mut written = 0u64;
        for i in 0..count {
            let plain = self.chunk(file, i)?;
            // Everything past the real size is padding.
            let keep = (size - written).min(plain.len() as u64) as usize;
            out.write_all(&plain[..keep])?;
            written += keep as u64;
        }
        Ok(written)
    }

    /// Upload `local` into `parent` as `name`, or as a new version of
    /// `existing`. Reads and encrypts one piece at a time.
    pub fn upload(
        &self,
        local: &Path,
        parent: &Entry,
        name: &str,
        existing: Option<&Entry>,
    ) -> Result<Entry> {
        let mut f = fs::File::open(local)?;
        let fmeta = f.metadata()?;
        let mtime = fmeta
            .modified()
            .ok()
            .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
            .map_or_else(now_ms, |d| d.as_millis() as i64);
        self.upload_from(&mut f, fmeta.len(), mtime, parent, name, existing)
    }

    /// Upload `size` bytes from `src`, like [`Client::upload`].
    pub fn upload_from(
        &self,
        src: &mut dyn Read,
        size: u64,
        mtime: i64,
        parent: &Entry,
        name: &str,
        existing: Option<&Entry>,
    ) -> Result<Entry> {
        let (node_id, node_key) = match existing {
            Some(e) => (e.node.id.clone(), e.key.clone()),
            None => (c::new_id(), Key::generate()),
        };
        let version_id = c::new_id();
        let ck = Key::generate();
        let padded = c::padded_size(size);
        let count = c::chunk_count(padded);
        let meta = Metadata {
            name: existing.map_or(name, |e| &e.meta.name).into(),
            mime: None,
            size,
            mtime,
            changed: Some(now_ms()),
            taken: None,
        };
        let req = CreateUploadRequest {
            node_id: node_id.clone(),
            parent_id: existing.is_none().then(|| parent.node.id.clone()),
            enc_key: existing
                .is_none()
                .then(|| B64(c::wrap_node_key(&parent.key, &node_key, &node_id))),
            enc_metadata: B64(c::encrypt_metadata(&node_key, &node_id, &meta)?),
            version_id: version_id.clone(),
            enc_content_key: B64(c::wrap_content_key(&node_key, &ck, &node_id, &version_id)),
            chunk_count: count,
            if_revision: existing.map(|e| e.node.revision),
            name_tag: existing
                .is_none()
                .then(|| B64(c::name_tag(&parent.key, &meta.name))),
        };
        let up: UploadResponse = self.post_json("/api/uploads", &req)?;
        let result = (|| -> Result<Node> {
            let mut buf = vec![0u8; CHUNK_SIZE];
            for i in 0..count {
                let want = (padded - i as u64 * CHUNK_SIZE as u64).min(CHUNK_SIZE as u64) as usize;
                let piece = &mut buf[..want];
                // Real bytes, then zeros up to the padded size.
                let mut got = 0;
                while got < want {
                    let n = src.read(&mut piece[got..])?;
                    if n == 0 {
                        break;
                    }
                    got += n;
                }
                piece[got..].fill(0);
                let enc = c::encrypt_chunk(&ck, &version_id, i, i + 1 == count, piece);
                let res = self
                    .agent
                    .put(self.url(&format!("/api/uploads/{}/chunks/{i}", up.upload_id)))
                    .header("Authorization", self.auth())
                    .header("Content-Type", "application/octet-stream")
                    .send(&enc[..])?;
                check(res)?;
            }
            Ok(self
                .send::<()>(
                    "POST",
                    &format!("/api/uploads/{}/finish", up.upload_id),
                    None,
                )?
                .body_mut()
                .read_json()?)
        })();
        match result {
            Ok(node) => Ok(Entry {
                node,
                key: node_key,
                meta,
            }),
            Err(e) => {
                let _ = self.send::<()>("DELETE", &format!("/api/uploads/{}", up.upload_id), None);
                Err(e)
            }
        }
    }

    /// Write everything under `folder` into an encrypted backup (see
    /// [`backup`]), under `key`. Calls `progress` with each path.
    pub fn backup(
        &self,
        folder: &Entry,
        out: impl Write,
        key: Key,
        progress: &mut dyn FnMut(&str),
    ) -> Result<SyncStats> {
        let mut w = backup::BackupWriter::new(out, key)?;
        let mut stats = SyncStats::default();
        let mut stack = vec![(folder.clone(), Vec::<String>::new())];
        while let Some((dir, path)) = stack.pop() {
            for e in self.list(&dir)? {
                let mut here = path.clone();
                here.push(e.meta.name.clone());
                progress(&here.join("/"));
                w.entry(&backup::BackupEntry {
                    path: here.clone(),
                    folder: e.is_folder(),
                    size: if e.is_folder() { 0 } else { e.meta.size },
                    mtime: e.meta.mtime,
                    mime: e.meta.mime.clone(),
                })?;
                if e.is_folder() {
                    stack.push((e, here));
                    continue;
                }
                let count = e.node.version.as_ref().map_or(0, |v| v.chunk_count);
                let mut left = e.meta.size;
                for i in 0..count {
                    if left == 0 {
                        break;
                    }
                    let plain = self.chunk(&e, i)?;
                    let keep = left.min(plain.len() as u64) as usize;
                    w.data(&plain[..keep])?;
                    left -= keep as u64;
                }
                if left != 0 {
                    return Err(Error::Usage(format!(
                        "{} is shorter than its size says",
                        here.join("/")
                    )));
                }
                stats.transferred += 1;
            }
        }
        w.finish()?;
        Ok(stats)
    }

    /// Restore a backup into `target`: folders are made (or reused, when
    /// one of that name is there), files uploaded, as new versions of files
    /// with the same name. Calls `progress` with each path.
    pub fn restore(
        &self,
        inp: impl Read,
        key: Key,
        target: &Entry,
        progress: &mut dyn FnMut(&str),
    ) -> Result<SyncStats> {
        let mut r = backup::BackupReader::new(inp, key)?;
        let mut stats = SyncStats::default();
        // Folders made or found so far, by path.
        let mut folders = std::collections::HashMap::new();
        folders.insert(Vec::<String>::new(), target.clone());
        while let Some(rec) = r.next_record()? {
            let backup::Record::Entry(e) = rec else {
                return Err(Error::Usage(
                    "this backup is damaged: data without a file".into(),
                ));
            };
            let Some((name, parent_path)) = e.path.split_last() else {
                return Err(Error::Usage("this backup is damaged: an empty path".into()));
            };
            let parent = folders.get(parent_path).cloned().ok_or_else(|| {
                Error::Usage("this backup is damaged: a file before its folder".into())
            })?;
            progress(&e.path.join("/"));
            let existing = self
                .list(&parent)?
                .into_iter()
                .find(|x| x.meta.name.to_lowercase() == name.to_lowercase());
            if e.folder {
                let dir = match existing {
                    Some(x) if x.is_folder() => x,
                    Some(_) => {
                        return Err(Error::Usage(format!(
                            "{} is a file here, and a folder in the backup",
                            e.path.join("/")
                        )));
                    }
                    None => self.mkdir(&parent, name)?,
                };
                folders.insert(e.path.clone(), dir);
                continue;
            }
            if existing.as_ref().is_some_and(Entry::is_folder) {
                return Err(Error::Usage(format!(
                    "{} is a folder here, and a file in the backup",
                    e.path.join("/")
                )));
            }
            let mut data = r.file_data(e.size);
            self.upload_from(&mut data, e.size, e.mtime, &parent, name, existing.as_ref())
                .map_err(|err| match err {
                    // A damaged backup shows up as a read error mid-upload.
                    Error::Io(io) => match io.into_inner().map(|b| b.downcast::<Error>()) {
                        Some(Ok(inner)) => *inner,
                        Some(Err(other)) => Error::Usage(other.to_string()),
                        None => Error::Usage("this backup couldn't be read".into()),
                    },
                    other => other,
                })?;
            stats.transferred += 1;
        }
        Ok(stats)
    }

    /// Mirror a remote folder into a local directory: new and changed files
    /// (by size and modification time) are downloaded, nothing is deleted.
    pub fn pull(
        &self,
        remote: &Entry,
        local: &Path,
        report: &mut dyn FnMut(&str),
    ) -> Result<SyncStats> {
        let mut stats = SyncStats::default();
        self.pull_into(remote, local, "", report, &mut stats)?;
        Ok(stats)
    }

    fn pull_into(
        &self,
        remote: &Entry,
        local: &Path,
        prefix: &str,
        report: &mut dyn FnMut(&str),
        stats: &mut SyncStats,
    ) -> Result<()> {
        fs::create_dir_all(local)?;
        for e in self.list(remote)? {
            let name = safe_name(&e.meta.name);
            let path = local.join(&name);
            let shown = format!("{prefix}{name}");
            if e.is_folder() {
                self.pull_into(&e, &path, &format!("{shown}/"), report, stats)?;
                continue;
            }
            if same_file(&path, e.meta.size, e.meta.mtime) {
                stats.unchanged += 1;
                continue;
            }
            report(&shown);
            let tmp = local.join(format!(".{name}.thencloud-part"));
            let mut out = fs::File::create(&tmp)?;
            let res = self.download(&e, &mut out);
            if let Err(err) = res {
                let _ = fs::remove_file(&tmp);
                return Err(err);
            }
            out.set_modified(UNIX_EPOCH + Duration::from_millis(e.meta.mtime.max(0) as u64))?;
            drop(out);
            fs::rename(&tmp, &path)?;
            stats.transferred += 1;
        }
        Ok(())
    }

    /// Mirror a local directory into a remote folder: new files are
    /// uploaded, changed ones (by size and modification time) get a new
    /// version, and missing folders are made. Nothing is deleted.
    pub fn push(
        &self,
        local: &Path,
        remote: &Entry,
        report: &mut dyn FnMut(&str),
    ) -> Result<SyncStats> {
        let mut stats = SyncStats::default();
        self.push_into(local, remote, "", report, &mut stats)?;
        Ok(stats)
    }

    fn push_into(
        &self,
        local: &Path,
        remote: &Entry,
        prefix: &str,
        report: &mut dyn FnMut(&str),
        stats: &mut SyncStats,
    ) -> Result<()> {
        let kids = self.list(remote)?;
        let mut items: Vec<_> = fs::read_dir(local)?.collect::<std::result::Result<_, _>>()?;
        items.sort_by_key(|d| d.file_name());
        for item in items {
            let Some(name) = item.file_name().to_str().map(String::from) else {
                continue;
            };
            if name.ends_with(".thencloud-part") {
                continue;
            }
            let shown = format!("{prefix}{name}");
            let there = kids
                .iter()
                .find(|k| k.meta.name.eq_ignore_ascii_case(&name));
            let ft = item.file_type()?;
            if ft.is_dir() {
                let folder = match there {
                    Some(f) if f.is_folder() => f.clone(),
                    Some(_) => {
                        return Err(Error::Usage(format!(
                            "{shown} is a file there but a folder here"
                        )));
                    }
                    None => self.mkdir(remote, &name)?,
                };
                self.push_into(&item.path(), &folder, &format!("{shown}/"), report, stats)?;
            } else if ft.is_file() {
                match there {
                    Some(f) if f.is_folder() => {
                        return Err(Error::Usage(format!(
                            "{shown} is a folder there but a file here"
                        )));
                    }
                    Some(f) if same_file(&item.path(), f.meta.size, f.meta.mtime) => {
                        stats.unchanged += 1
                    }
                    other => {
                        report(&shown);
                        self.upload(&item.path(), remote, &name, other)?;
                        stats.transferred += 1;
                    }
                }
            }
        }
        Ok(())
    }
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct SyncStats {
    pub transferred: usize,
    pub unchanged: usize,
}

pub(crate) fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as i64)
}

/// A local file matches if it has the same size and, to the second, the
/// same modification time.
fn same_file(path: &Path, size: u64, mtime_ms: i64) -> bool {
    let Ok(m) = fs::metadata(path) else {
        return false;
    };
    let local = m
        .modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map_or(-1, |d| d.as_millis() as i64);
    m.is_file() && m.len() == size && local / 1000 == mtime_ms / 1000
}

/// A name that's safe as one path segment on disk: names come from whoever
/// uploaded the file, so "../x" must not escape the target directory.
/// Percent-encodes everything but unreserved characters, for a query value.
fn url_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

pub fn safe_name(name: &str) -> String {
    let clean: String = name
        .chars()
        .map(|ch| {
            if ch == '/' || ch == '\\' || ch == '\0' {
                '_'
            } else {
                ch
            }
        })
        .collect();
    match clean.trim() {
        "" | "." | ".." => "_".into(),
        _ => clean,
    }
}
