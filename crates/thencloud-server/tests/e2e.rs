//! End-to-end tests: a native client built on `thencloud-crypto` talks to an
//! in-process server. Also asserts that no plaintext (file content, file
//! names, passwords, keys) ever lands in the database or blob store.

use std::path::Path;

use axum::Router;
use axum::body::Body;
use axum::http::{HeaderMap, Method, Request, StatusCode};
use http_body_util::BodyExt;
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::json;
use thencloud_crypto::api::*;
use thencloud_crypto::{self as c, KdfParams, Key, KeyPair, Metadata};
use thencloud_server::{AppState, Config, router};
use tower::ServiceExt;

const FAST_KDF: KdfParams = KdfParams {
    m_cost: 19 * 1024,
    t_cost: 2,
    p_cost: 1,
};
const MARKER: &[u8] = b"TOP-SECRET-PAYLOAD-";

struct Harness {
    app: Router,
    state: AppState,
    dir: tempfile::TempDir,
}

struct Resp {
    status: StatusCode,
    body: Vec<u8>,
    headers: HeaderMap,
}

impl std::fmt::Debug for Resp {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} {}", self.status, String::from_utf8_lossy(&self.body))
    }
}

impl Resp {
    fn json<T: DeserializeOwned>(&self) -> T {
        serde_json::from_slice(&self.body)
            .unwrap_or_else(|e| panic!("bad json ({e}): {}", String::from_utf8_lossy(&self.body)))
    }
    fn error(&self) -> String {
        self.json::<ErrorBody>().error
    }
}

impl Harness {
    async fn new() -> Self {
        Self::with_config(|_| {}).await
    }

    async fn with_config(f: impl FnOnce(&mut Config)) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let mut cfg = Config::for_dir(dir.path());
        f(&mut cfg);
        let state = AppState::new(cfg).await.unwrap();
        Harness {
            app: router(state.clone()),
            state,
            dir,
        }
    }

    async fn raw(
        &self,
        method: Method,
        uri: &str,
        token: Option<&str>,
        headers: &[(&str, &str)],
        body: Body,
        ct: Option<&str>,
    ) -> Resp {
        let mut b = Request::builder().method(method).uri(uri);
        if let Some(t) = token {
            b = b.header("authorization", format!("Bearer {t}"));
        }
        if let Some(ct) = ct {
            b = b.header("content-type", ct);
        }
        for (k, v) in headers {
            b = b.header(*k, *v);
        }
        let res = self
            .app
            .clone()
            .oneshot(b.body(body).unwrap())
            .await
            .unwrap();
        let status = res.status();
        let headers = res.headers().clone();
        let body = res.into_body().collect().await.unwrap().to_bytes().to_vec();
        Resp {
            status,
            body,
            headers,
        }
    }

    async fn call(
        &self,
        method: Method,
        uri: &str,
        token: Option<&str>,
        body: Option<impl Serialize>,
    ) -> Resp {
        match body {
            Some(b) => {
                let bytes = serde_json::to_vec(&b).unwrap();
                self.raw(
                    method,
                    uri,
                    token,
                    &[],
                    Body::from(bytes),
                    Some("application/json"),
                )
                .await
            }
            None => self.raw(method, uri, token, &[], Body::empty(), None).await,
        }
    }

    async fn get(&self, uri: &str, token: &str) -> Resp {
        self.call(Method::GET, uri, Some(token), None::<()>).await
    }
}

/// A logged-in client holding decrypted keys in memory.
struct Client {
    username: String,
    token: String,
    mk: Key,
    kp: KeyPair,
    root: String,
}

/// What to seal to: both halves of their key when they have both.
fn sealing_key(u: &UserPublicKey) -> Vec<u8> {
    let mut k = u.public_key.0.clone();
    if let Some(pq) = &u.pq_public_key {
        k.extend_from_slice(pq);
    }
    k
}

fn meta(name: &str, size: u64) -> Metadata {
    Metadata {
        name: name.into(),
        mime: Some("application/octet-stream".into()),
        size,
        mtime: 1_700_000_000_000,
        changed: None,
        taken: None,
    }
}

async fn register(h: &Harness, username: &str, password: &str) -> Client {
    match try_register(h, username, password, None).await {
        Ok(c) => c,
        Err(r) => panic!(
            "register failed: {} {}",
            r.status,
            String::from_utf8_lossy(&r.body)
        ),
    }
}

async fn try_register(
    h: &Harness,
    username: &str,
    password: &str,
    invite: Option<&str>,
) -> Result<Client, Box<Resp>> {
    let salt = c::random_bytes(c::SALT_LEN);
    let ak = c::derive_account_keys(password, &salt, FAST_KDF).unwrap();
    let mk = Key::generate();
    let kp = KeyPair::generate().with_pq(c::PqKeyPair::generate());
    let pq = kp.pq.as_ref().unwrap();
    let root_id = c::new_id();
    let root_key = Key::generate();
    let req = RegisterRequest {
        username: username.into(),
        auth_key: B64(ak.auth_key.as_bytes().to_vec()),
        kdf_salt: B64(salt),
        kdf_params: FAST_KDF,
        enc_master_key: B64(c::wrap_master_key(&ak.kek, &mk)),
        public_key: B64(kp.public.to_vec()),
        enc_private_key: B64(c::wrap_private_key(&mk, &kp.secret)),
        pq_public_key: Some(B64(pq.public.clone())),
        enc_pq_private_key: Some(B64(c::wrap_pq_private_key(&mk, pq))),
        root: NewRootFolder {
            id: root_id.clone(),
            enc_key: B64(c::wrap_node_key(&mk, &root_key, &root_id)),
            enc_metadata: B64(c::encrypt_metadata(&root_key, &root_id, &meta("root", 0)).unwrap()),
        },
        device_name: Some("test".into()),
        invite: invite.map(Into::into),
    };
    let r = h
        .call(Method::POST, "/api/auth/register", None, Some(&req))
        .await;
    if r.status != StatusCode::CREATED {
        return Err(Box::new(r));
    }
    let s: SessionResponse = r.json();
    Ok(Client {
        username: username.into(),
        token: s.token,
        mk,
        kp,
        root: root_id,
    })
}

async fn login(h: &Harness, username: &str, password: &str) -> Result<Client, Box<Resp>> {
    let pre: PreloginResponse = h
        .call(
            Method::POST,
            "/api/auth/prelogin",
            None,
            Some(json!({"username": username})),
        )
        .await
        .json();
    let ak = c::derive_account_keys(password, &pre.kdf_salt, pre.kdf_params).unwrap();
    let req = LoginRequest {
        username: username.into(),
        auth_key: B64(ak.auth_key.as_bytes().to_vec()),
        device_name: None,
    };
    let r = h
        .call(Method::POST, "/api/auth/login", None, Some(&req))
        .await;
    if r.status != StatusCode::OK {
        return Err(Box::new(r));
    }
    let s: SessionResponse = r.json();
    let mk = c::unwrap_master_key(&ak.kek, &s.me.keys.enc_master_key).unwrap();
    let mut kp = c::unwrap_private_key(&mk, &s.me.keys.enc_private_key).unwrap();
    if let Some(w) = &s.me.keys.enc_pq_private_key {
        kp = kp.with_pq(c::unwrap_pq_private_key(&mk, w).unwrap());
    }
    Ok(Client {
        username: username.into(),
        token: s.token,
        mk,
        kp,
        root: s.me.keys.root_node_id,
    })
}

impl Client {
    /// Derive a node's key the way the web client does: via /path.
    async fn key_of(&self, h: &Harness, node_id: &str) -> Key {
        let r = h
            .get(&format!("/api/nodes/{node_id}/path"), &self.token)
            .await;
        assert_eq!(r.status, StatusCode::OK);
        let p: NodePath = r.json();
        let first = &p.nodes[0];
        let mut key = match &p.share {
            Some(s) => c::open_share_key(&self.kp, &s.wrapped_key, &first.id).unwrap(),
            None => c::unwrap_node_key(&self.mk, &first.enc_key, &first.id).unwrap(),
        };
        for n in &p.nodes[1..] {
            key = c::unwrap_node_key(&key, &n.enc_key, &n.id).unwrap();
        }
        key
    }

    async fn mkdir(&self, h: &Harness, parent: &str, name: &str) -> (String, Key) {
        let pk = self.key_of(h, parent).await;
        let id = c::new_id();
        let k = Key::generate();
        let req = CreateFolderRequest {
            id: id.clone(),
            parent_id: parent.into(),
            enc_key: B64(c::wrap_node_key(&pk, &k, &id)),
            enc_metadata: B64(c::encrypt_metadata(&k, &id, &meta(name, 0)).unwrap()),
            name_tag: Some(B64(c::name_tag(&pk, name))),
        };
        let r = h
            .call(
                Method::POST,
                "/api/nodes/folder",
                Some(&self.token),
                Some(&req),
            )
            .await;
        assert_eq!(
            r.status,
            StatusCode::CREATED,
            "{}",
            String::from_utf8_lossy(&r.body)
        );
        (id, k)
    }

    /// Upload a new file (`existing` = None) or a new version. Returns the
    /// status of the first failing step, or the resulting node.
    async fn upload(
        &self,
        h: &Harness,
        parent: &str,
        existing: Option<&Node>,
        name: &str,
        data: &[u8],
    ) -> Result<Node, Box<Resp>> {
        let (node_id, node_key, enc_key, name_tag) = match existing {
            Some(n) => (n.id.clone(), self.key_of(h, &n.id).await, None, None),
            None => {
                let pk = self.key_of(h, parent).await;
                let id = c::new_id();
                let k = Key::generate();
                let ek = B64(c::wrap_node_key(&pk, &k, &id));
                (id, k, Some(ek), Some(B64(c::name_tag(&pk, name))))
            }
        };
        let version_id = c::new_id();
        let ck = Key::generate();
        let chunks = c::encrypt_content(&ck, &version_id, data);
        let req = CreateUploadRequest {
            node_id: node_id.clone(),
            parent_id: existing.is_none().then(|| parent.to_string()),
            enc_key,
            enc_metadata: B64(c::encrypt_metadata(
                &node_key,
                &node_id,
                &meta(name, data.len() as u64),
            )
            .unwrap()),
            version_id: version_id.clone(),
            enc_content_key: B64(c::wrap_content_key(&node_key, &ck, &node_id, &version_id)),
            chunk_count: chunks.len() as u32,
            if_revision: existing.map(|n| n.revision),
            name_tag,
        };
        let r = h
            .call(Method::POST, "/api/uploads", Some(&self.token), Some(&req))
            .await;
        if r.status != StatusCode::CREATED {
            return Err(Box::new(r));
        }
        let up: UploadResponse = r.json();
        // Upload out of order to exercise index handling.
        for (i, chunk) in chunks.iter().enumerate().rev() {
            let r = h
                .raw(
                    Method::PUT,
                    &format!("/api/uploads/{}/chunks/{i}", up.upload_id),
                    Some(&self.token),
                    &[],
                    Body::from(chunk.clone()),
                    Some("application/octet-stream"),
                )
                .await;
            if r.status != StatusCode::NO_CONTENT {
                return Err(Box::new(r));
            }
        }
        let r = h
            .call(
                Method::POST,
                &format!("/api/uploads/{}/finish", up.upload_id),
                Some(&self.token),
                None::<()>,
            )
            .await;
        if r.status != StatusCode::OK {
            return Err(Box::new(r));
        }
        Ok(r.json())
    }

    async fn download(&self, h: &Harness, node: &Node, node_key: &Key) -> (Metadata, Vec<u8>) {
        download_via(
            h,
            node,
            node_key,
            |i| format!("/api/nodes/{}/chunks/{i}", node.id),
            Some(&self.token),
            &[],
        )
        .await
    }

    async fn children(&self, h: &Harness, folder: &str) -> Resp {
        h.get(&format!("/api/nodes/{folder}/children"), &self.token)
            .await
    }

    async fn trash(&self, h: &Harness) -> Vec<TrashItem> {
        let r = h.get("/api/trash", &self.token).await;
        assert_eq!(r.status, StatusCode::OK);
        r.json()
    }

    async fn delete(&self, h: &Harness, id: &str) -> StatusCode {
        h.call(
            Method::DELETE,
            &format!("/api/nodes/{id}"),
            Some(&self.token),
            None::<()>,
        )
        .await
        .status
    }

    async fn versions(&self, h: &Harness, id: &str) -> Vec<FileVersion> {
        let r = h
            .get(&format!("/api/nodes/{id}/versions"), &self.token)
            .await;
        assert_eq!(r.status, StatusCode::OK, "{r:?}");
        r.json()
    }

    async fn me(&self, h: &Harness) -> Me {
        h.get("/api/me", &self.token).await.json()
    }
}

async fn download_via(
    h: &Harness,
    node: &Node,
    node_key: &Key,
    url: impl Fn(u32) -> String,
    token: Option<&str>,
    headers: &[(&str, &str)],
) -> (Metadata, Vec<u8>) {
    let m = c::decrypt_metadata(node_key, &node.id, &node.enc_metadata).unwrap();
    let v = node.version.as_ref().expect("file has a version");
    let ck = c::unwrap_content_key(node_key, &v.enc_content_key, &node.id, &v.id).unwrap();
    let mut out = Vec::new();
    for i in 0..v.chunk_count {
        let r = h
            .raw(Method::GET, &url(i), token, headers, Body::empty(), None)
            .await;
        assert_eq!(
            r.status,
            StatusCode::OK,
            "{}",
            String::from_utf8_lossy(&r.body)
        );
        assert_eq!(r.headers.get("x-version-id").unwrap(), v.id.as_str());
        out.extend(c::decrypt_chunk(&ck, &v.id, i, i + 1 == v.chunk_count, &r.body).unwrap());
    }
    // Padding after the real size is dropped.
    assert!(out.len() as u64 >= m.size);
    out.truncate(m.size as usize);
    (m, out)
}

fn names(nodes: &[Node], parent_key: &Key) -> Vec<String> {
    let mut v: Vec<String> = nodes
        .iter()
        .map(|n| {
            let k = c::unwrap_node_key(parent_key, &n.enc_key, &n.id).unwrap();
            c::decrypt_metadata(&k, &n.id, &n.enc_metadata)
                .unwrap()
                .name
        })
        .collect();
    v.sort();
    v
}

/// What a browser sends for a link with a password, and the secret that
/// goes after `#`. Upload-only links derive from the fragment they carry
/// (the owner's identity) and wrap nothing.
fn link_with_password(node_id: &str, node_key: &Key, password: &str) -> (CreateLinkRequest, Key) {
    let secret = Key::generate();
    let k = c::derive_link_password_keys(secret.as_bytes(), password).unwrap();
    let req = CreateLinkRequest {
        node_id: node_id.into(),
        password_auth: Some(B64(k.auth_key.as_bytes().to_vec())),
        enc_link_key: Some(B64(c::wrap_link_key(&k.kek, node_key, node_id))),
        enc_link_secret: Some(B64(c::encrypt_link_secret(node_key, &secret, node_id))),
        expires_at: None,
        upload_only: false,
        max_opens: None,
    };
    (req, secret)
}

/// The body of an unlock request: the auth key, never the password.
fn unlock_body(secret: &Key, password: &str) -> serde_json::Value {
    let k = c::derive_link_password_keys(secret.as_bytes(), password).unwrap();
    json!({ "auth": k.auth_key.to_b64() })
}

fn find_by_name<'a>(nodes: &'a [Node], parent_key: &Key, name: &str) -> &'a Node {
    nodes
        .iter()
        .find(|n| {
            let k = c::unwrap_node_key(parent_key, &n.enc_key, &n.id).unwrap();
            c::decrypt_metadata(&k, &n.id, &n.enc_metadata)
                .unwrap()
                .name
                == name
        })
        .unwrap_or_else(|| panic!("{name} not found"))
}

fn secret_payload(len: usize) -> Vec<u8> {
    MARKER.iter().copied().cycle().take(len).collect()
}

fn all_files(dir: &Path, out: &mut Vec<std::path::PathBuf>) {
    for e in std::fs::read_dir(dir).unwrap() {
        let p = e.unwrap().path();
        if p.is_dir() {
            all_files(&p, out)
        } else {
            out.push(p)
        }
    }
}

fn contains(hay: &[u8], needle: &[u8]) -> bool {
    hay.windows(needle.len()).any(|w| w == needle)
}

#[tokio::test]
async fn full_lifecycle_is_zero_knowledge() {
    let h = Harness::new().await;
    let alice = register(&h, "alice", "correct horse battery staple").await;
    let bob = register(&h, "Bob", "hunter2-but-longer").await;
    assert!(alice.me(&h).await.is_admin);
    assert!(!bob.me(&h).await.is_admin);
    assert_eq!(bob.username, "Bob");
    assert_eq!(bob.me(&h).await.username, "bob");

    // --- folders and a multi-chunk file ------------------------------------
    let (docs, docs_key) = alice.mkdir(&h, &alice.root, "Tax Documents 2026").await;
    let (other, other_key) = alice.mkdir(&h, &alice.root, "Holiday Photos").await;
    let data = secret_payload(c::CHUNK_SIZE * 2 + 12345);
    let file = alice
        .upload(&h, &docs, None, "secret-plan.txt", &data)
        .await
        .unwrap();
    assert_eq!(file.version.as_ref().unwrap().chunk_count, 3);

    let kids: Vec<Node> = alice.children(&h, &docs).await.json();
    assert_eq!(names(&kids, &docs_key), vec!["secret-plan.txt"]);
    let file_key = c::unwrap_node_key(&docs_key, &kids[0].enc_key, &kids[0].id).unwrap();
    let (m, got) = alice.download(&h, &kids[0], &file_key).await;
    assert_eq!(m.name, "secret-plan.txt");
    assert_eq!(got, data);

    // --- rename + move (re-wrap under the new parent) -----------------------
    let r = h
        .call(
            Method::PATCH,
            &format!("/api/nodes/{}", file.id),
            Some(&alice.token),
            Some(UpdateNodeRequest {
                enc_metadata: Some(B64(c::encrypt_metadata(
                    &file_key,
                    &file.id,
                    &meta("renamed-plan.txt", data.len() as u64),
                )
                .unwrap())),
                parent_id: Some(other.clone()),
                enc_key: Some(B64(c::wrap_node_key(&other_key, &file_key, &file.id))),
                if_revision: Some(file.revision),
                name_tag: Some(B64(c::name_tag(&other_key, "renamed-plan.txt"))),
            }),
        )
        .await;
    assert_eq!(
        r.status,
        StatusCode::OK,
        "{}",
        String::from_utf8_lossy(&r.body)
    );
    let moved: Node = r.json();
    assert_eq!(moved.revision, file.revision + 1);
    assert!(
        alice
            .children(&h, &docs)
            .await
            .json::<Vec<Node>>()
            .is_empty()
    );
    // A stale revision is rejected.
    let r = h
        .call(
            Method::PATCH,
            &format!("/api/nodes/{}", file.id),
            Some(&alice.token),
            Some(UpdateNodeRequest {
                enc_metadata: Some(B64(
                    c::encrypt_metadata(&file_key, &file.id, &meta("x", 0)).unwrap()
                )),
                if_revision: Some(file.revision),
                ..Default::default()
            }),
        )
        .await;
    assert_eq!(r.status, StatusCode::CONFLICT);
    // Moving a folder into its own subtree is rejected.
    let (sub, sub_key) = alice.mkdir(&h, &docs, "sub").await;
    let _ = sub_key;
    let r = h
        .call(
            Method::PATCH,
            &format!("/api/nodes/{docs}"),
            Some(&alice.token),
            Some(UpdateNodeRequest {
                parent_id: Some(sub.clone()),
                enc_key: Some(B64(c::wrap_node_key(&Key::generate(), &docs_key, &docs))),
                ..Default::default()
            }),
        )
        .await;
    assert_eq!(r.status, StatusCode::BAD_REQUEST);

    // --- bob can't see anything yet ------------------------------------------
    assert_eq!(bob.children(&h, &other).await.status, StatusCode::NOT_FOUND);
    assert_eq!(
        h.get(&format!("/api/nodes/{}/chunks/0", file.id), &bob.token)
            .await
            .status,
        StatusCode::NOT_FOUND
    );

    // --- share "Holiday Photos" with bob, read-only -------------------------
    let pk: UserPublicKey = h
        .get("/api/users/bob/public-key", &alice.token)
        .await
        .json();
    assert_eq!(
        c::fingerprint(&pk.public_key),
        c::fingerprint(&bob.kp.public)
    );
    let share_req = CreateShareRequest {
        node_id: other.clone(),
        recipient: "bob".into(),
        wrapped_key: B64(c::seal_share_key(&sealing_key(&pk), &other_key, &other).unwrap()),
        permission: Permission::Read,
        expires_at: None,
    };
    let r = h
        .call(
            Method::POST,
            "/api/shares",
            Some(&alice.token),
            Some(&share_req),
        )
        .await;
    assert_eq!(r.status, StatusCode::CREATED);
    let share: OutgoingShare = r.json();

    let incoming: Vec<IncomingShare> = h.get("/api/shares/incoming", &bob.token).await.json();
    assert_eq!(incoming.len(), 1);
    assert_eq!(incoming[0].owner, "alice");
    let shared_key =
        c::open_share_key(&bob.kp, &incoming[0].wrapped_key, &incoming[0].node.id).unwrap();
    let meta_shared =
        c::decrypt_metadata(&shared_key, &other, &incoming[0].node.enc_metadata).unwrap();
    assert_eq!(meta_shared.name, "Holiday Photos");
    let kids: Vec<Node> = bob.children(&h, &other).await.json();
    let f = find_by_name(&kids, &shared_key, "renamed-plan.txt");
    let fk = bob.key_of(&h, &f.id).await; // via /path + share key
    let (_, got) = bob.download(&h, f, &fk).await;
    assert_eq!(got, data);

    // read-only: no uploads, no deleting
    let err = bob
        .upload(&h, &other, None, "from-bob.txt", b"hi alice")
        .await
        .unwrap_err();
    assert_eq!(err.status, StatusCode::FORBIDDEN);
    let r = h
        .call(
            Method::DELETE,
            &format!("/api/nodes/{}", f.id),
            Some(&bob.token),
            None::<()>,
        )
        .await;
    assert_eq!(r.status, StatusCode::FORBIDDEN);

    // --- upgrade to write; bob uploads into alice's folder ------------------
    let r = h
        .call(
            Method::PATCH,
            &format!("/api/shares/{}", share.id),
            Some(&alice.token),
            Some(UpdateShareRequest {
                permission: Permission::Write,
            }),
        )
        .await;
    assert_eq!(r.status, StatusCode::NO_CONTENT);
    let bob_file = bob
        .upload(&h, &other, None, "from-bob.txt", b"hi alice")
        .await
        .unwrap();
    assert_eq!(
        bob_file.owner, "alice",
        "files in a shared tree are owned (and billed) to the tree owner"
    );
    let kids: Vec<Node> = alice.children(&h, &other).await.json();
    assert_eq!(
        names(&kids, &other_key),
        vec!["from-bob.txt", "renamed-plan.txt"]
    );
    let bk = c::unwrap_node_key(&other_key, &bob_file.enc_key, &bob_file.id).unwrap();
    assert_eq!(alice.download(&h, &bob_file, &bk).await.1, b"hi alice");
    // ...but can't delete or rename the shared folder itself
    let r = h
        .call(
            Method::DELETE,
            &format!("/api/nodes/{other}"),
            Some(&bob.token),
            None::<()>,
        )
        .await;
    assert_eq!(r.status, StatusCode::FORBIDDEN);
    // ...and can't share it onwards
    let r = h
        .call(
            Method::POST,
            "/api/shares",
            Some(&bob.token),
            Some(&share_req),
        )
        .await;
    assert_eq!(r.status, StatusCode::FORBIDDEN);

    // bob's /path view starts at the shared folder
    let p: NodePath = h
        .get(&format!("/api/nodes/{}/path", bob_file.id), &bob.token)
        .await
        .json();
    assert_eq!(
        p.nodes.iter().map(|n| n.id.as_str()).collect::<Vec<_>>(),
        vec![other.as_str(), bob_file.id.as_str()]
    );
    assert_eq!(p.share.unwrap().permission, Permission::Write);

    // --- public link with password -----------------------------------------
    let (req, secret) = link_with_password(&other, &other_key, "letmein");
    // Every part of it goes together.
    let mut half = req.clone();
    half.enc_link_key = None;
    let r = h
        .call(Method::POST, "/api/links", Some(&alice.token), Some(half))
        .await;
    assert_eq!(r.status, StatusCode::BAD_REQUEST);
    let r = h
        .call(Method::POST, "/api/links", Some(&alice.token), Some(req))
        .await;
    assert_eq!(r.status, StatusCode::CREATED);
    let link: Link = r.json();
    assert!(link.has_password);
    // The owner can show the link again: the secret is sealed under the node key.
    let listed: Vec<Link> = h
        .get(&format!("/api/links?node_id={other}"), &alice.token)
        .await
        .json();
    let again = listed.iter().find(|l| l.id == link.id).unwrap();
    assert!(
        c::decrypt_link_secret(&other_key, again.enc_link_secret.as_ref().unwrap(), &other)
            .unwrap()
            == secret
    );
    // What the browser would have: /s/<token>#p.<secret>. Only the token is sent.
    let base = format!("/api/public/{}", link.token);
    let r = h
        .raw(Method::GET, &base, None, &[], Body::empty(), None)
        .await;
    assert_eq!(r.status, StatusCode::UNAUTHORIZED);
    assert_eq!(r.error(), "password_required");
    let r = h
        .call(
            Method::POST,
            &format!("{base}/unlock"),
            None,
            Some(unlock_body(&secret, "nope")),
        )
        .await;
    assert_eq!(r.status, StatusCode::UNAUTHORIZED);
    let r = h
        .call(
            Method::POST,
            &format!("{base}/unlock"),
            None,
            Some(unlock_body(&secret, "letmein")),
        )
        .await;
    assert_eq!(r.status, StatusCode::OK);
    let unlocked: UnlockLinkResponse = r.json();
    let lt = [("x-link-token", unlocked.link_token.as_str())];
    let info: PublicLinkInfo = h
        .raw(Method::GET, &base, None, &lt, Body::empty(), None)
        .await
        .json();
    // The node key comes wrapped under the password and the secret: without
    // either, it doesn't open.
    let wrapped = info.enc_link_key.unwrap();
    let info_node = info.node.unwrap();
    let wrong = c::derive_link_password_keys(secret.as_bytes(), "nope").unwrap();
    assert!(c::unwrap_link_key(&wrong.kek, &wrapped, &info_node.id).is_err());
    let k = c::derive_link_password_keys(secret.as_bytes(), "letmein").unwrap();
    let lk = c::unwrap_link_key(&k.kek, &wrapped, &info_node.id).unwrap();
    assert!(lk == other_key);
    assert_eq!(
        c::decrypt_metadata(&lk, &info_node.id, &info_node.enc_metadata)
            .unwrap()
            .name,
        "Holiday Photos"
    );
    let r = h
        .raw(
            Method::GET,
            &format!("{base}/nodes/{other}/children"),
            None,
            &lt,
            Body::empty(),
            None,
        )
        .await;
    let kids: Vec<Node> = r.json();
    let f = find_by_name(&kids, &lk, "renamed-plan.txt");
    let fk = c::unwrap_node_key(&lk, &f.enc_key, &f.id).unwrap();
    let (_, got) = download_via(
        &h,
        f,
        &fk,
        |i| format!("{base}/nodes/{}/chunks/{i}", f.id),
        None,
        &lt,
    )
    .await;
    assert_eq!(got, data);
    // Nodes outside the linked subtree are not reachable through the link.
    let r = h
        .raw(
            Method::GET,
            &format!("{base}/nodes/{docs}/children"),
            None,
            &lt,
            Body::empty(),
            None,
        )
        .await;
    assert_eq!(r.status, StatusCode::NOT_FOUND);
    // A forged link token is rejected.
    let forged = [("x-link-token", "99999999999.AAAA")];
    let r = h
        .raw(Method::GET, &base, None, &forged, Body::empty(), None)
        .await;
    assert_eq!(r.status, StatusCode::UNAUTHORIZED);

    // The change feed has recorded all that (ids only; scanned below).
    let feed: ChangeFeed = h.get("/api/changes?since=0", &alice.token).await.json();
    assert!(feed.changes.iter().any(|c| c.node_id == file.id));

    // --- zero-knowledge check: no plaintext at rest -------------------------
    let mut files = Vec::new();
    all_files(&h.dir.path().join("data"), &mut files);
    assert!(files.iter().any(|p| p.to_string_lossy().contains("blobs")));
    let needles: Vec<&[u8]> = vec![
        MARKER,
        b"secret-plan",
        b"renamed-plan",
        b"from-bob",
        b"hi alice",
        b"Holiday Photos",
        b"Tax Documents",
        b"correct horse",
        b"hunter2",
        b"letmein",
        alice.mk.as_bytes(),
        other_key.as_bytes(),
        file_key.as_bytes(),
        alice.kp.secret.as_bytes(),
    ];
    for p in &files {
        let bytes = std::fs::read(p).unwrap();
        for n in &needles {
            assert!(
                !contains(&bytes, n),
                "plaintext {:?} found in {}",
                String::from_utf8_lossy(n),
                p.display()
            );
        }
    }

    // --- a new version becomes current; the old one stays in the history -----
    let used_before = alice.me(&h).await.used_bytes;
    let moved_now: Node = h
        .get(&format!("/api/nodes/{}", file.id), &alice.token)
        .await
        .json();
    let old_vid = moved_now.version.as_ref().unwrap().id.clone();
    let v2 = alice
        .upload(&h, "", Some(&moved_now), "renamed-plan.txt", b"short now")
        .await
        .unwrap();
    assert_ne!(v2.version.as_ref().unwrap().id, old_vid);
    assert!(alice.me(&h).await.used_bytes > used_before);
    let blob_dir = h
        .dir
        .path()
        .join("data/blobs")
        .join(&old_vid[..2])
        .join(&old_vid);
    assert!(blob_dir.exists(), "old version blobs are kept");
    // Uploading on top of a stale revision fails.
    let err = alice
        .upload(&h, "", Some(&moved_now), "renamed-plan.txt", b"stale")
        .await
        .unwrap_err();
    assert_eq!(err.status, StatusCode::CONFLICT);
    let (_, got) = alice.download(&h, &v2, &file_key).await;
    assert_eq!(got, b"short now");

    // --- revoke + delete ----------------------------------------------------
    let r = h
        .call(
            Method::DELETE,
            &format!("/api/shares/{}", share.id),
            Some(&alice.token),
            None::<()>,
        )
        .await;
    assert_eq!(r.status, StatusCode::NO_CONTENT);
    assert_eq!(bob.children(&h, &other).await.status, StatusCode::NOT_FOUND);
    assert!(
        h.get("/api/shares/incoming", &bob.token)
            .await
            .json::<Vec<IncomingShare>>()
            .is_empty()
    );

    let r = h
        .call(
            Method::DELETE,
            &format!("/api/links/{}", link.id),
            Some(&alice.token),
            None::<()>,
        )
        .await;
    assert_eq!(r.status, StatusCode::NO_CONTENT);
    assert_eq!(
        h.raw(Method::GET, &base, None, &lt, Body::empty(), None)
            .await
            .status,
        StatusCode::NOT_FOUND
    );

    let used = alice.me(&h).await.used_bytes;
    for folder in [&other, &docs] {
        let r = h
            .call(
                Method::DELETE,
                &format!("/api/nodes/{folder}"),
                Some(&alice.token),
                None::<()>,
            )
            .await;
        assert_eq!(r.status, StatusCode::NO_CONTENT);
    }
    // Deleted items sit in the trash (still counted) until it's emptied.
    assert_eq!(alice.me(&h).await.used_bytes, used);
    assert_eq!(alice.trash(&h).await.len(), 2);
    let r = h
        .call(Method::DELETE, "/api/trash", Some(&alice.token), None::<()>)
        .await;
    assert_eq!(r.status, StatusCode::NO_CONTENT);
    assert!(alice.trash(&h).await.is_empty());
    assert_eq!(alice.me(&h).await.used_bytes, 0);
    let mut files = Vec::new();
    all_files(&h.dir.path().join("data/blobs"), &mut files);
    assert!(files.is_empty(), "all blobs removed: {files:?}");
    // The root can't be deleted.
    let r = h
        .call(
            Method::DELETE,
            &format!("/api/nodes/{}", alice.root),
            Some(&alice.token),
            None::<()>,
        )
        .await;
    assert_eq!(r.status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn login_password_change_and_enumeration() {
    let h = Harness::new().await;
    let a = register(&h, "carol", "first password").await;

    // Login recovers the same master key.
    let again = login(&h, "carol", "first password").await.ok().unwrap();
    assert!(again.mk == a.mk);
    assert_eq!(again.kp.public, a.kp.public);
    let err = login(&h, "carol", "wrong").await.err().unwrap();
    assert_eq!(err.status, StatusCode::UNAUTHORIZED);

    // Prelogin for unknown users is stable and looks like a real answer.
    let p1: PreloginResponse = h
        .call(
            Method::POST,
            "/api/auth/prelogin",
            None,
            Some(json!({"username": "nobody"})),
        )
        .await
        .json();
    let p2: PreloginResponse = h
        .call(
            Method::POST,
            "/api/auth/prelogin",
            None,
            Some(json!({"username": "nobody"})),
        )
        .await
        .json();
    assert_eq!(p1.kdf_salt, p2.kdf_salt);
    assert_eq!(p1.kdf_salt.len(), c::SALT_LEN);

    // Change password: MK is re-wrapped, other sessions are signed out.
    let pre: PreloginResponse = h
        .call(
            Method::POST,
            "/api/auth/prelogin",
            None,
            Some(json!({"username": "carol"})),
        )
        .await
        .json();
    let cur = c::derive_account_keys("first password", &pre.kdf_salt, pre.kdf_params).unwrap();
    let salt = c::random_bytes(c::SALT_LEN);
    let new = c::derive_account_keys("second password", &salt, FAST_KDF).unwrap();
    let req = ChangePasswordRequest {
        current_auth_key: B64(cur.auth_key.as_bytes().to_vec()),
        new_auth_key: B64(new.auth_key.as_bytes().to_vec()),
        new_kdf_salt: B64(salt),
        new_kdf_params: FAST_KDF,
        new_enc_master_key: B64(c::wrap_master_key(&new.kek, &a.mk)),
    };
    let r = h
        .call(
            Method::POST,
            "/api/auth/password",
            Some(&a.token),
            Some(&req),
        )
        .await;
    assert_eq!(r.status, StatusCode::NO_CONTENT);
    assert_eq!(
        h.get("/api/me", &again.token).await.status,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(h.get("/api/me", &a.token).await.status, StatusCode::OK);
    assert!(login(&h, "carol", "first password").await.is_err());
    let after = login(&h, "carol", "second password").await.ok().unwrap();
    assert!(after.mk == a.mk);

    // Logout kills the session.
    let r = h
        .call(
            Method::POST,
            "/api/auth/logout",
            Some(&after.token),
            None::<()>,
        )
        .await;
    assert_eq!(r.status, StatusCode::NO_CONTENT);
    assert_eq!(
        h.get("/api/me", &after.token).await.status,
        StatusCode::UNAUTHORIZED
    );

    // Duplicate usernames are rejected (case-insensitively).
    let salt = c::random_bytes(c::SALT_LEN);
    let ak = c::derive_account_keys("x", &salt, FAST_KDF).unwrap();
    let r = h
        .call(
            Method::POST,
            "/api/auth/register",
            None,
            Some(json!({
                "username": "CAROL", "auth_key": ak.auth_key.to_b64(), "kdf_salt": c::b64_encode(&salt),
                "kdf_params": FAST_KDF, "enc_master_key": c::b64_encode(&[0; 72]), "public_key": c::b64_encode(&[0; 32]),
                "enc_private_key": c::b64_encode(&[0; 72]),
                "root": {"id": c::new_id(), "enc_key": c::b64_encode(&[0; 72]), "enc_metadata": c::b64_encode(&[0; 64])}
            })),
        )
        .await;
    assert_eq!(r.status, StatusCode::CONFLICT);
}

#[tokio::test]
async fn login_is_rate_limited() {
    let h = Harness::new().await;
    register(&h, "dave", "pw").await;
    for _ in 0..10 {
        assert_eq!(
            login(&h, "dave", "bad").await.err().unwrap().status,
            StatusCode::UNAUTHORIZED
        );
    }
    assert_eq!(
        login(&h, "dave", "pw").await.err().unwrap().status,
        StatusCode::TOO_MANY_REQUESTS
    );
}

#[tokio::test]
async fn quota_is_enforced_and_uploads_can_be_aborted() {
    let h = Harness::with_config(|c| c.default_quota = 1024).await;
    let e = register(&h, "erin", "pw").await;
    let err = e
        .upload(&h, &e.root, None, "big.bin", &[7u8; 2048])
        .await
        .unwrap_err();
    assert_eq!(err.status, StatusCode::INSUFFICIENT_STORAGE);
    assert_eq!(e.me(&h).await.used_bytes, 0);
    e.upload(&h, &e.root, None, "small.bin", &[7u8; 100])
        .await
        .unwrap();
    assert!(e.me(&h).await.used_bytes > 100);

    // An abandoned upload is reclaimed by the janitor after it expires.
    let h2 = Harness::with_config(|c| c.upload_ttl_hours = 0).await;
    let f = register(&h2, "frank", "pw").await;
    let err = f
        .upload(&h2, &f.root, None, "x", b"data")
        .await
        .unwrap_err();
    assert_eq!(
        err.status,
        StatusCode::NOT_FOUND,
        "ttl 0: upload expires immediately"
    );
    thencloud_server::janitor::run_once(&h2.state)
        .await
        .unwrap();
    let uploads: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM uploads")
        .fetch_one(&h2.state.db)
        .await
        .unwrap();
    assert_eq!(uploads, 0);
}

#[tokio::test]
async fn registration_can_be_closed_but_first_user_is_allowed() {
    let h = Harness::with_config(|c| c.allow_registration = false).await;
    register(&h, "admin", "pw").await;
    let salt = c::random_bytes(c::SALT_LEN);
    let r = h
        .call(Method::POST, "/api/auth/register", None, Some(json!({
            "username": "mallory", "auth_key": c::b64_encode(&[1; 32]), "kdf_salt": c::b64_encode(&salt),
            "kdf_params": FAST_KDF, "enc_master_key": c::b64_encode(&[0; 72]), "public_key": c::b64_encode(&[0; 32]),
            "enc_private_key": c::b64_encode(&[0; 72]),
            "root": {"id": c::new_id(), "enc_key": c::b64_encode(&[0; 72]), "enc_metadata": c::b64_encode(&[0; 64])}
        })))
        .await;
    assert_eq!(r.status, StatusCode::FORBIDDEN);
    assert_eq!(r.error(), "registration_closed");
}

#[tokio::test]
async fn security_headers_are_set() {
    let h = Harness::new().await;
    let r = h
        .raw(Method::GET, "/api/nope", None, &[], Body::empty(), None)
        .await;
    assert_eq!(r.status, StatusCode::NOT_FOUND);
    assert_eq!(r.headers.get("referrer-policy").unwrap(), "no-referrer");
    assert!(
        r.headers
            .get("content-security-policy")
            .unwrap()
            .to_str()
            .unwrap()
            .contains("default-src 'self'")
    );
    assert_eq!(r.headers.get("cache-control").unwrap(), "no-store");

    // Pages are always revalidated; hashed assets are cached for good, but
    // only when they exist.
    let web = h.dir.path().join("web");
    std::fs::create_dir_all(web.join("assets")).unwrap();
    std::fs::write(web.join("index.html"), "<!doctype html>").unwrap();
    std::fs::write(web.join("assets/app-abc123.css"), "body{}").unwrap();
    let get = |uri: &'static str| {
        let h = &h;
        async move {
            h.raw(Method::GET, uri, None, &[], Body::empty(), None)
                .await
        }
    };
    let page = get("/").await;
    assert_eq!(page.status, StatusCode::OK);
    assert_eq!(page.headers.get("cache-control").unwrap(), "no-cache");
    let asset = get("/assets/app-abc123.css").await;
    assert_eq!(asset.status, StatusCode::OK);
    assert!(
        asset
            .headers
            .get("cache-control")
            .unwrap()
            .to_str()
            .unwrap()
            .contains("immutable")
    );
    // Precompressed copies are used when the browser accepts them.
    std::fs::write(
        web.join("assets/app-abc123.css.gz"),
        b"\x1f\x8bnot-really-gzip",
    )
    .unwrap();
    let gz = h
        .raw(
            Method::GET,
            "/assets/app-abc123.css",
            None,
            &[("accept-encoding", "gzip")],
            Body::empty(),
            None,
        )
        .await;
    assert_eq!(gz.headers.get("content-encoding").unwrap(), "gzip");
    assert_eq!(&gz.body[..2], b"\x1f\x8b");
    // Brotli wins when both are there and accepted.
    std::fs::write(web.join("assets/app-abc123.css.br"), b"not-really-brotli").unwrap();
    let br = h
        .raw(
            Method::GET,
            "/assets/app-abc123.css",
            None,
            &[("accept-encoding", "gzip, br")],
            Body::empty(),
            None,
        )
        .await;
    assert_eq!(br.headers.get("content-encoding").unwrap(), "br");
    assert_eq!(br.headers.get("vary").unwrap(), "accept-encoding");
    assert_eq!(&br.body[..], b"not-really-brotli");
    let missing = get("/assets/app-gone.css").await;
    assert_eq!(missing.status, StatusCode::NOT_FOUND);
    assert_eq!(missing.headers.get("cache-control").unwrap(), "no-cache");
}

/// Download a specific (possibly old) version of a file.
async fn download_version(
    h: &Harness,
    c: &Client,
    node: &Node,
    key: &Key,
    v: &FileVersion,
) -> Vec<u8> {
    let as_node = Node {
        enc_metadata: v.enc_metadata.clone(),
        version: Some(VersionInfo {
            id: v.id.clone(),
            enc_content_key: v.enc_content_key.clone(),
            chunk_count: v.chunk_count,
            size: v.size,
            created_at: v.created_at,
            has_thumbnail: false,
        }),
        ..node.clone()
    };
    let (_, data) = download_via(
        h,
        &as_node,
        key,
        |i| format!("/api/nodes/{}/versions/{}/chunks/{i}", node.id, v.id),
        Some(&c.token),
        &[],
    )
    .await;
    data
}

fn blob_exists(h: &Harness, version_id: &str) -> bool {
    h.dir
        .path()
        .join("data/blobs")
        .join(&version_id[..2])
        .join(version_id)
        .exists()
}

#[tokio::test]
async fn version_history_restore_and_limits() {
    let h = Harness::with_config(|c| c.max_versions = 3).await;
    let alice = register(&h, "alice", "pw").await;
    let bob = register(&h, "bob", "pw").await;

    let f = alice
        .upload(&h, &alice.root, None, "doc.txt", b"first")
        .await
        .unwrap();
    let key = alice.key_of(&h, &f.id).await;
    let f = alice
        .upload(&h, "", Some(&f), "doc.txt", b"second!")
        .await
        .unwrap();
    let f = alice
        .upload(&h, "", Some(&f), "doc.txt", b"third!!!")
        .await
        .unwrap();

    let vs = alice.versions(&h, &f.id).await;
    assert_eq!(vs.len(), 3);
    assert!(vs[0].current, "newest first, and it's current");
    assert!(vs[1..].iter().all(|v| !v.current));
    assert_eq!(vs[0].created_by, "alice");
    // Each version decrypts to its own content, with its own metadata.
    assert_eq!(
        download_version(&h, &alice, &f, &key, &vs[2]).await,
        b"first"
    );
    assert_eq!(
        download_version(&h, &alice, &f, &key, &vs[1]).await,
        b"second!"
    );

    // Restore the first version: the client re-encrypts the node metadata
    // with the current name and that version's size.
    let first = vs[2].clone();
    let r = h
        .call(
            Method::POST,
            &format!("/api/nodes/{}/versions/{}/restore", f.id, first.id),
            Some(&alice.token),
            Some(RestoreVersionRequest {
                enc_metadata: B64(c::encrypt_metadata(&key, &f.id, &meta("doc.txt", 5)).unwrap()),
                if_revision: Some(f.revision),
            }),
        )
        .await;
    assert_eq!(r.status, StatusCode::OK, "{r:?}");
    let f: Node = r.json();
    assert_eq!(f.version.as_ref().unwrap().id, first.id);
    assert_eq!(alice.download(&h, &f, &key).await.1, b"first");
    let vs = alice.versions(&h, &f.id).await;
    assert_eq!(vs.len(), 3, "restoring doesn't add or drop versions");
    assert!(vs.iter().find(|v| v.id == first.id).unwrap().current);

    // A fourth upload pushes out the oldest upload (pruning goes by upload
    // time, even for a version that was restored in between).
    let oldest_non_current = vs.last().unwrap().id.clone();
    let f = alice
        .upload(&h, "", Some(&f), "doc.txt", b"fourth")
        .await
        .unwrap();
    let vs = alice.versions(&h, &f.id).await;
    assert_eq!(vs.len(), 3);
    assert!(!vs.iter().any(|v| v.id == oldest_non_current));
    assert!(!blob_exists(&h, &oldest_non_current));

    // Share read-only with bob: he can see and download history, not change it.
    let pk: UserPublicKey = h
        .get("/api/users/bob/public-key", &alice.token)
        .await
        .json();
    let r = h
        .call(
            Method::POST,
            "/api/shares",
            Some(&alice.token),
            Some(CreateShareRequest {
                node_id: f.id.clone(),
                recipient: "bob".into(),
                wrapped_key: B64(c::seal_share_key(&sealing_key(&pk), &key, &f.id).unwrap()),
                permission: Permission::Read,
                expires_at: None,
            }),
        )
        .await;
    assert_eq!(r.status, StatusCode::CREATED);
    let bvs = bob.versions(&h, &f.id).await;
    assert_eq!(bvs.len(), 3);
    let old = bvs.iter().find(|v| !v.current).unwrap();
    let bob_key = bob.key_of(&h, &f.id).await;
    download_version(&h, &bob, &f, &bob_key, old).await;
    let r = h
        .call(
            Method::DELETE,
            &format!("/api/nodes/{}/versions/{}", f.id, old.id),
            Some(&bob.token),
            None::<()>,
        )
        .await;
    assert_eq!(r.status, StatusCode::FORBIDDEN);

    // The owner can delete old versions (freeing space), not the current one.
    let used = alice.me(&h).await.used_bytes;
    let r = h
        .call(
            Method::DELETE,
            &format!("/api/nodes/{}/versions/{}", f.id, old.id),
            Some(&alice.token),
            None::<()>,
        )
        .await;
    assert_eq!(r.status, StatusCode::NO_CONTENT);
    assert_eq!(alice.me(&h).await.used_bytes, used - old.size);
    assert!(!blob_exists(&h, &old.id));
    let current = alice
        .versions(&h, &f.id)
        .await
        .into_iter()
        .find(|v| v.current)
        .unwrap();
    let r = h
        .call(
            Method::DELETE,
            &format!("/api/nodes/{}/versions/{}", f.id, current.id),
            Some(&alice.token),
            None::<()>,
        )
        .await;
    assert_eq!(r.status, StatusCode::BAD_REQUEST);
}

/// Upload a file through an upload-only link the way the share page does:
/// the node key is sealed to the owner's public key from the fragment.
async fn drop_file(
    h: &Harness,
    token: &str,
    owner_pub: &[u8],
    folder: &str,
    name: &str,
    data: &[u8],
) -> (String, Key, StatusCode) {
    let id = c::new_id();
    let k = Key::generate();
    let version_id = c::new_id();
    let ck = Key::generate();
    let chunks = c::encrypt_content(&ck, &version_id, data);
    let req = CreateUploadRequest {
        node_id: id.clone(),
        parent_id: Some(folder.into()),
        enc_key: Some(B64(c::seal_drop_key(owner_pub, &k, &id, folder).unwrap())),
        enc_metadata: B64(c::encrypt_metadata(&k, &id, &meta(name, data.len() as u64)).unwrap()),
        version_id: version_id.clone(),
        enc_content_key: B64(c::wrap_content_key(&k, &ck, &id, &version_id)),
        chunk_count: chunks.len() as u32,
        if_revision: None,
        name_tag: None,
    };
    let base = format!("/api/public/{token}/uploads");
    let r = h.call(Method::POST, &base, None, Some(&req)).await;
    if r.status != StatusCode::CREATED {
        return (id, k, r.status);
    }
    let up: UploadResponse = r.json();
    for (i, chunk) in chunks.iter().enumerate() {
        let r = h
            .raw(
                Method::PUT,
                &format!("{base}/{}/chunks/{i}", up.upload_id),
                None,
                &[],
                Body::from(chunk.clone()),
                Some("application/octet-stream"),
            )
            .await;
        if r.status != StatusCode::NO_CONTENT {
            return (id, k, r.status);
        }
    }
    let r = h
        .call(
            Method::POST,
            &format!("{base}/{}/finish", up.upload_id),
            None,
            None::<()>,
        )
        .await;
    (id, k, r.status)
}

#[tokio::test]
async fn file_drop_links_are_upload_only_and_zero_knowledge() {
    let h = Harness::new().await;
    let alice = register(&h, "alice", "pw").await;
    let bob = register(&h, "bob", "pw").await;
    let (inbox, inbox_key) = alice.mkdir(&h, &alice.root, "Inbox").await;
    let file = alice
        .upload(&h, &inbox, None, "existing.txt", b"already here")
        .await
        .unwrap();
    let mk_link = |node_id: String, upload_only: bool| CreateLinkRequest {
        node_id,
        password_auth: None,
        enc_link_key: None,
        enc_link_secret: None,
        expires_at: None,
        upload_only,
        max_opens: None,
    };
    let r = h
        .call(
            Method::POST,
            "/api/links",
            Some(&alice.token),
            Some(mk_link(file.id.clone(), true)),
        )
        .await;
    assert_eq!(r.status, StatusCode::BAD_REQUEST, "only folders take drops");
    let r = h
        .call(
            Method::POST,
            "/api/links",
            Some(&alice.token),
            Some(mk_link(inbox.clone(), true)),
        )
        .await;
    assert_eq!(r.status, StatusCode::CREATED);
    let link: Link = r.json();
    assert!(link.upload_only);
    let listed: Vec<Link> = h.get("/api/links", &alice.token).await.json();
    assert!(listed[0].upload_only);

    // The visitor learns who it goes to, and nothing about the folder.
    let base = format!("/api/public/{}", link.token);
    let info: PublicLinkInfo = h
        .raw(Method::GET, &base, None, &[], Body::empty(), None)
        .await
        .json();
    assert!(info.upload_only && info.node.is_none());
    assert_eq!(info.owner.as_deref(), Some("alice"));
    assert_eq!(info.folder_id.as_deref(), Some(inbox.as_str()));
    for uri in [
        format!("{base}/nodes/{inbox}/children"),
        format!("{base}/nodes/{}/chunks/0", file.id),
    ] {
        let r = h
            .raw(Method::GET, &uri, None, &[], Body::empty(), None)
            .await;
        assert_eq!(r.status, StatusCode::FORBIDDEN, "{uri}");
    }

    // Drop a file. The fragment carries alice's public key.
    let alice_pub = alice.kp.public.to_vec();
    let secret: Vec<u8> = [MARKER, b"dropped content"].concat();
    let (dropped, dk, st) = drop_file(
        &h,
        &link.token,
        &alice_pub,
        &inbox,
        "drop-secret-name.pdf",
        &secret,
    )
    .await;
    assert_eq!(st, StatusCode::NO_CONTENT);
    // Only into the linked folder itself.
    let (_, _, st) = drop_file(&h, &link.token, &alice_pub, &alice.root, "x", b"x").await;
    assert_eq!(st, StatusCode::FORBIDDEN);
    // Not through a normal link.
    let plain: Link = h
        .call(
            Method::POST,
            "/api/links",
            Some(&alice.token),
            Some(mk_link(inbox.clone(), false)),
        )
        .await
        .json();
    let (_, _, st) = drop_file(&h, &plain.token, &alice_pub, &inbox, "x", b"x").await;
    assert_eq!(st, StatusCode::FORBIDDEN);

    // Until alice takes it in, it's hidden everywhere.
    let kids: Vec<Node> = h
        .get(&format!("/api/nodes/{inbox}/children"), &alice.token)
        .await
        .json();
    assert_eq!(kids.len(), 1);
    let r = h.get(&format!("/api/nodes/{dropped}"), &alice.token).await;
    assert_eq!(r.status, StatusCode::NOT_FOUND);
    let none: Vec<DroppedFile> = h.get("/api/drops", &bob.token).await.json();
    assert!(none.is_empty());

    let drops: Vec<DroppedFile> = h.get("/api/drops", &alice.token).await.json();
    assert_eq!(drops.len(), 1);
    let d = &drops[0];
    assert_eq!(d.node.id, dropped);
    let parent = d.node.parent_id.clone().unwrap();
    assert_eq!(parent, inbox);
    let k = c::open_drop_key(&alice.kp, &d.sealed_key, &d.node.id, &parent).unwrap();
    assert!(k == dk);
    let wrapped = B64(c::wrap_node_key(&inbox_key, &k, &d.node.id));
    // Taken in under a fresh node key, so the visitor's key stops mattering.
    let v = d.node.version.clone().unwrap();
    let ck = c::unwrap_content_key(&k, &v.enc_content_key, &d.node.id, &v.id).unwrap();
    let fresh = Key::generate();
    let m = c::decrypt_metadata(&k, &d.node.id, &d.node.enc_metadata).unwrap();
    let r = h
        .call(
            Method::POST,
            &format!("/api/drops/{dropped}/adopt"),
            Some(&bob.token),
            Some(AdoptDropRequest {
                enc_key: wrapped,
                enc_metadata: None,
                name_tag: None,
                enc_content_key: None,
            }),
        )
        .await;
    assert_eq!(r.status, StatusCode::NOT_FOUND);
    let r = h
        .call(
            Method::POST,
            &format!("/api/drops/{dropped}/adopt"),
            Some(&alice.token),
            Some(AdoptDropRequest {
                enc_key: B64(c::wrap_node_key(&inbox_key, &fresh, &d.node.id)),
                enc_metadata: Some(B64(c::encrypt_metadata(&fresh, &d.node.id, &m).unwrap())),
                name_tag: Some(B64(c::name_tag(&inbox_key, "drop-secret-name.pdf"))),
                enc_content_key: Some(B64(c::wrap_content_key(&fresh, &ck, &d.node.id, &v.id))),
            }),
        )
        .await;
    assert_eq!(r.status, StatusCode::OK, "{r:?}");
    assert!(alice.key_of(&h, &dropped).await == fresh);
    let kids: Vec<Node> = h
        .get(&format!("/api/nodes/{inbox}/children"), &alice.token)
        .await
        .json();
    let node = find_by_name(&kids, &inbox_key, "drop-secret-name.pdf");
    let key = alice.key_of(&h, &node.id).await;
    assert_eq!(alice.download(&h, node, &key).await.1, secret);
    let drops: Vec<DroppedFile> = h.get("/api/drops", &alice.token).await.json();
    assert!(drops.is_empty());

    // A drop can also be thrown away, releasing its space.
    let used = alice.me(&h).await.used_bytes;
    let (junk, _, st) = drop_file(&h, &link.token, &alice_pub, &inbox, "junk", &[7; 500]).await;
    assert_eq!(st, StatusCode::NO_CONTENT);
    assert!(alice.me(&h).await.used_bytes > used);
    let r = h
        .call(
            Method::DELETE,
            &format!("/api/drops/{junk}"),
            Some(&alice.token),
            None::<()>,
        )
        .await;
    assert_eq!(r.status, StatusCode::NO_CONTENT);
    assert_eq!(alice.me(&h).await.used_bytes, used);

    // An unfinished drop upload gives its space back when the visitor
    // aborts it, and when the link is deleted.
    let start_drop = |token: String| {
        let h = &h;
        let inbox = inbox.clone();
        let alice_pub = alice_pub.clone();
        async move {
            let id = c::new_id();
            let k = Key::generate();
            let vid = c::new_id();
            let ck = Key::generate();
            let chunks = c::encrypt_content(&ck, &vid, &[1u8; 3000]);
            let req = CreateUploadRequest {
                node_id: id.clone(),
                parent_id: Some(inbox.clone()),
                enc_key: Some(B64(c::seal_drop_key(&alice_pub, &k, &id, &inbox).unwrap())),
                enc_metadata: B64(c::encrypt_metadata(&k, &id, &meta("half", 3000)).unwrap()),
                version_id: vid.clone(),
                enc_content_key: B64(c::wrap_content_key(&k, &ck, &id, &vid)),
                chunk_count: chunks.len() as u32,
                if_revision: None,
                name_tag: None,
            };
            let base = format!("/api/public/{token}/uploads");
            let up: UploadResponse = h.call(Method::POST, &base, None, Some(&req)).await.json();
            let r = h
                .raw(
                    Method::PUT,
                    &format!("{base}/{}/chunks/0", up.upload_id),
                    None,
                    &[],
                    Body::from(chunks[0].clone()),
                    Some("application/octet-stream"),
                )
                .await;
            assert_eq!(r.status, StatusCode::NO_CONTENT);
            format!("{base}/{}", up.upload_id)
        }
    };
    let uri = start_drop(link.token.clone()).await;
    assert!(alice.me(&h).await.used_bytes > used);
    let r = h.call(Method::DELETE, &uri, None, None::<()>).await;
    assert_eq!(r.status, StatusCode::NO_CONTENT);
    assert_eq!(alice.me(&h).await.used_bytes, used);
    start_drop(link.token.clone()).await;
    assert!(alice.me(&h).await.used_bytes > used);
    let r = h
        .call(
            Method::DELETE,
            &format!("/api/links/{}", link.id),
            Some(&alice.token),
            None::<()>,
        )
        .await;
    assert_eq!(r.status, StatusCode::NO_CONTENT);
    assert_eq!(alice.me(&h).await.used_bytes, used);
    let left: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM uploads")
        .fetch_one(&h.state.db)
        .await
        .unwrap();
    assert_eq!(left, 0);

    // --- zero-knowledge check ----------------------------------------------
    let mut files = Vec::new();
    all_files(&h.dir.path().join("data"), &mut files);
    let needles: Vec<&[u8]> = vec![
        MARKER,
        b"drop-secret-name",
        b"dropped content",
        dk.as_bytes(),
        inbox_key.as_bytes(),
    ];
    for p in &files {
        let bytes = std::fs::read(p).unwrap();
        for n in &needles {
            assert!(
                !contains(&bytes, n),
                "plaintext {:?} found in {}",
                String::from_utf8_lossy(n),
                p.display()
            );
        }
    }
}

async fn current_auth(h: &Harness, password: &str) -> Key {
    let pre: PreloginResponse = h
        .call(
            Method::POST,
            "/api/auth/prelogin",
            None,
            Some(json!({"username": "alice"})),
        )
        .await
        .json();
    c::derive_account_keys(password, &pre.kdf_salt, pre.kdf_params)
        .unwrap()
        .auth_key
}

/// Sign in the way a sync client would, with only the app password text.
async fn app_login(h: &Harness, text: String) -> (Resp, Key) {
    let secret = c::decode_recovery_key(&text).unwrap();
    let keys = c::derive_app_password_keys(&secret);
    let r = h
        .call(
            Method::POST,
            "/api/auth/app-login",
            None,
            Some(AppLoginRequest {
                auth_key: B64(keys.auth_key.as_bytes().to_vec()),
                device_name: Some("sync client".into()),
            }),
        )
        .await;
    (r, keys.kek)
}

#[tokio::test]
async fn app_passwords_are_scoped_revocable_and_opaque() {
    let h = Harness::new().await;
    let alice = register(&h, "alice", "correct horse").await;
    let file = alice
        .upload(&h, &alice.root, None, "notes.txt", b"sync me")
        .await
        .unwrap();

    // Made in the browser: 32 random bytes, split into auth key and KEK.
    let make = |scope: AppScope, name: &str, password: &str| {
        let secret = Key::generate();
        let text = c::encode_recovery_key(&secret);
        let keys = c::derive_app_password_keys(&secret);
        let id = c::new_id();
        let pre = c::derive_account_keys(password, &[0u8; 16], FAST_KDF).unwrap();
        (
            text,
            id.clone(),
            CreateAppPasswordRequest {
                id: id.clone(),
                name: name.into(),
                scope,
                current_auth_key: B64(pre.auth_key.as_bytes().to_vec()),
                auth_key: B64(keys.auth_key.as_bytes().to_vec()),
                enc_master_key: B64(c::wrap_master_key_app(&keys.kek, &alice.mk, &id)),
            },
        )
    };
    // The account password is required.
    let (_, _, mut req) = make(AppScope::Full, "laptop sync", "x");
    req.current_auth_key = B64(current_auth(&h, "wrong").await.as_bytes().to_vec());
    let r = h
        .call(
            Method::POST,
            "/api/app-passwords",
            Some(&alice.token),
            Some(&req),
        )
        .await;
    assert_eq!(r.status, StatusCode::UNAUTHORIZED);

    let mut created = Vec::new();
    for (scope, name) in [
        (AppScope::Full, "laptop sync"),
        (AppScope::Read, "backup box"),
    ] {
        let (text, id, mut req) = make(scope, name, "x");
        req.current_auth_key = B64(current_auth(&h, "correct horse").await.as_bytes().to_vec());
        let r = h
            .call(
                Method::POST,
                "/api/app-passwords",
                Some(&alice.token),
                Some(&req),
            )
            .await;
        assert_eq!(r.status, StatusCode::CREATED, "{r:?}");
        created.push((text, id));
    }
    let listed: Vec<AppPassword> = h.get("/api/app-passwords", &alice.token).await.json();
    assert_eq!(listed.len(), 2);
    assert!(listed.iter().all(|a| a.last_used_at.is_none()));

    // A client signs in with the app password alone and unwraps the master key.
    let (r, kek) = app_login(&h, created[0].0.clone()).await;
    assert_eq!(r.status, StatusCode::OK, "{r:?}");
    let full: AppLoginResponse = r.json();
    assert_eq!(full.scope, AppScope::Full);
    let mk = c::unwrap_master_key_app(&kek, &full.enc_master_key, &full.app_password_id).unwrap();
    assert!(mk == alice.mk);
    assert!(c::unwrap_master_key_app(&kek, &full.enc_master_key, &created[1].1).is_err());
    let r = h.get(&format!("/api/nodes/{}", file.id), &full.token).await;
    assert_eq!(r.status, StatusCode::OK);
    // App sessions can't make more app passwords.
    let (_, _, mut req) = make(AppScope::Full, "sneaky", "x");
    req.current_auth_key = B64(current_auth(&h, "correct horse").await.as_bytes().to_vec());
    let r = h
        .call(
            Method::POST,
            "/api/app-passwords",
            Some(&full.token),
            Some(&req),
        )
        .await;
    assert_eq!(r.status, StatusCode::FORBIDDEN);

    // An app session can't revoke other app passwords or sign out other devices.
    let r = h
        .call(
            Method::DELETE,
            &format!("/api/app-passwords/{}", created[1].1),
            Some(&full.token),
            None::<()>,
        )
        .await;
    assert_eq!(r.status, StatusCode::FORBIDDEN);
    let r = h
        .call(
            Method::DELETE,
            "/api/sessions",
            Some(&full.token),
            None::<()>,
        )
        .await;
    assert_eq!(r.status, StatusCode::FORBIDDEN);
    let others: Vec<DeviceSession> = h.get("/api/sessions", &full.token).await.json();
    let browser = others.iter().find(|s| s.app_password.is_none()).unwrap();
    let r = h
        .call(
            Method::DELETE,
            &format!("/api/sessions/{}", browser.id),
            Some(&full.token),
            None::<()>,
        )
        .await;
    assert_eq!(r.status, StatusCode::NOT_FOUND);

    // Read-only: can list and download, not change anything.
    let (r, _) = app_login(&h, created[1].0.clone()).await;
    let ro: AppLoginResponse = r.json();
    assert_eq!(ro.scope, AppScope::Read);
    let r = h
        .get(&format!("/api/nodes/{}/chunks/0", file.id), &ro.token)
        .await;
    assert_eq!(r.status, StatusCode::OK);
    let r = h
        .call(
            Method::DELETE,
            &format!("/api/nodes/{}", file.id),
            Some(&ro.token),
            None::<()>,
        )
        .await;
    assert_eq!(r.status, StatusCode::FORBIDDEN);
    let sessions: Vec<DeviceSession> = h.get("/api/sessions", &alice.token).await.json();
    assert!(
        sessions
            .iter()
            .any(|s| s.app_password.as_deref() == Some("backup box"))
    );

    // A wrong app password is refused.
    let (r, _) = app_login(&h, c::encode_recovery_key(&Key::generate())).await;
    assert_eq!(r.status, StatusCode::UNAUTHORIZED);

    // Revoking signs its sessions out; the other one keeps working.
    let r = h
        .call(
            Method::DELETE,
            &format!("/api/app-passwords/{}", created[0].1),
            Some(&alice.token),
            None::<()>,
        )
        .await;
    assert_eq!(r.status, StatusCode::NO_CONTENT);
    assert_eq!(
        h.get("/api/me", &full.token).await.status,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(h.get("/api/me", &ro.token).await.status, StatusCode::OK);
    let (r, _) = app_login(&h, created[0].0.clone()).await;
    assert_eq!(r.status, StatusCode::UNAUTHORIZED);

    // Nothing that opens the master key is stored.
    let mut files = Vec::new();
    all_files(&h.dir.path().join("data"), &mut files);
    let secret = c::decode_recovery_key(&created[1].0).unwrap();
    let keys = c::derive_app_password_keys(&secret);
    for p in &files {
        let bytes = std::fs::read(p).unwrap();
        for n in [
            secret.as_bytes().as_slice(),
            keys.kek.as_bytes(),
            keys.auth_key.as_bytes(),
            created[1].0.as_bytes(),
            alice.mk.as_bytes(),
        ] {
            assert!(!contains(&bytes, n), "secret found in {}", p.display());
        }
    }
}

#[tokio::test]
async fn drafts_are_per_user_writers_only_and_opaque() {
    let h = Harness::new().await;
    let alice = register(&h, "alice", "pw").await;
    let bob = register(&h, "bob", "pw").await;
    let (folder, folder_key) = alice.mkdir(&h, &alice.root, "Notes").await;
    let file = alice
        .upload(&h, &folder, None, "plan.md", b"# Plan")
        .await
        .unwrap();
    let alice_id = alice.me(&h).await.user_id;
    let label = format!("draft:{}", file.id);
    let text = b"# Plan\n\nDRAFT-SECRET-WORDS";
    let draft = Draft {
        data: B64(c::encrypt_private_data(&alice.mk, &alice_id, &label, text)),
        base_revision: file.revision,
        updated_at: 0,
    };
    let uri = format!("/api/nodes/{}/draft", file.id);
    let none: Option<Draft> = h.get(&uri, &alice.token).await.json();
    assert!(none.is_none());
    let r = h
        .call(Method::PUT, &uri, Some(&alice.token), Some(&draft))
        .await;
    assert_eq!(r.status, StatusCode::NO_CONTENT, "{r:?}");
    let got: Draft = h
        .get(&uri, &alice.token)
        .await
        .json::<Option<Draft>>()
        .unwrap();
    assert_eq!(got.base_revision, file.revision);
    assert_eq!(
        c::decrypt_private_data(&alice.mk, &alice_id, &label, &got.data).unwrap(),
        text
    );
    // Bound to the file: it doesn't open as another file's draft.
    assert!(c::decrypt_private_data(&alice.mk, &alice_id, "draft:other", &got.data).is_err());

    // Read-only recipients can't keep drafts; writers get their own.
    let pk: UserPublicKey = h
        .get("/api/users/bob/public-key", &alice.token)
        .await
        .json();
    let share = |permission| CreateShareRequest {
        node_id: folder.clone(),
        recipient: "bob".into(),
        wrapped_key: B64(c::seal_share_key(&sealing_key(&pk), &folder_key, &folder).unwrap()),
        permission,
        expires_at: None,
    };
    let r = h
        .call(
            Method::POST,
            "/api/shares",
            Some(&alice.token),
            Some(share(Permission::Read)),
        )
        .await;
    assert_eq!(r.status, StatusCode::CREATED);
    let r = h
        .call(Method::PUT, &uri, Some(&bob.token), Some(&draft))
        .await;
    assert_eq!(r.status, StatusCode::FORBIDDEN);
    let s: Vec<OutgoingShare> = h.get("/api/shares/outgoing", &alice.token).await.json();
    h.call(
        Method::PATCH,
        &format!("/api/shares/{}", s[0].id),
        Some(&alice.token),
        Some(json!({"permission": "write"})),
    )
    .await;
    assert!(
        h.get(&uri, &bob.token)
            .await
            .json::<Option<Draft>>()
            .is_none()
    );
    let r = h
        .call(Method::PUT, &uri, Some(&bob.token), Some(&draft))
        .await;
    assert_eq!(r.status, StatusCode::NO_CONTENT);

    // Too large is refused.
    let big = Draft {
        data: B64(vec![0; 5 * 1024 * 1024 + 512 * 1024]),
        base_revision: 1,
        updated_at: 0,
    };
    let r = h
        .call(Method::PUT, &uri, Some(&alice.token), Some(&big))
        .await;
    assert_eq!(r.status, StatusCode::BAD_REQUEST);

    // No plaintext at rest.
    let mut files = Vec::new();
    all_files(&h.dir.path().join("data"), &mut files);
    for p in &files {
        assert!(
            !contains(&std::fs::read(p).unwrap(), b"DRAFT-SECRET-WORDS"),
            "draft found in {}",
            p.display()
        );
    }

    // Discarding, and deleting the file, remove drafts.
    let r = h
        .call(Method::DELETE, &uri, Some(&alice.token), None::<()>)
        .await;
    assert_eq!(r.status, StatusCode::NO_CONTENT);
    assert!(
        h.get(&uri, &alice.token)
            .await
            .json::<Option<Draft>>()
            .is_none()
    );
    assert!(
        h.get(&uri, &bob.token)
            .await
            .json::<Option<Draft>>()
            .is_some()
    );
    assert_eq!(alice.delete(&h, &file.id).await, StatusCode::NO_CONTENT);
    let r = h
        .call(
            Method::DELETE,
            &format!("/api/trash/{}", file.id),
            Some(&alice.token),
            None::<()>,
        )
        .await;
    assert_eq!(r.status, StatusCode::NO_CONTENT);
    let left: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM drafts")
        .fetch_one(&h.state.db)
        .await
        .unwrap();
    assert_eq!(left, 0);
}

#[tokio::test]
async fn profile_pictures_and_display_names_are_encrypted_and_only_for_share_partners() {
    let h = Harness::new().await;
    let alice = register(&h, "alice", "pw").await;
    let bob = register(&h, "bob", "pw").await;
    let carol = register(&h, "carol", "pw").await;
    let alice_id = alice.me(&h).await.user_id;
    let picture = b"AVATAR-PNG-SECRET-PIXELS";
    let display_name = "Alice Pleasance Secretname";
    let pronouns = c::PersonDetails {
        subject: "zesecret".into(),
        object: "zirsecret".into(),
        possessive: "zirs".into(),
        gender: Some(c::Gender::Feminine),
    };
    let ak = Key::generate();
    let enc_key = B64(c::encrypt_private_data(
        &alice.mk,
        &alice_id,
        "avatar-key",
        ak.as_bytes(),
    ));
    // Nothing to set, or a name that isn't the fixed padded size: refused.
    for bad in [
        SetAvatar {
            data: None,
            name: None,
            details: None,
            enc_key: enc_key.clone(),
        },
        SetAvatar {
            data: None,
            name: Some(B64(c::encrypt_avatar(&ak, "alice", b"short"))),
            details: None,
            enc_key: enc_key.clone(),
        },
        SetAvatar {
            data: None,
            name: None,
            details: Some(B64(c::encrypt_avatar(&ak, "alice", b"short"))),
            enc_key: enc_key.clone(),
        },
    ] {
        let r = h
            .call(Method::PUT, "/api/me/avatar", Some(&alice.token), Some(bad))
            .await;
        assert_eq!(r.status, StatusCode::BAD_REQUEST, "{r:?}");
    }
    let r = h
        .call(
            Method::PUT,
            "/api/me/avatar",
            Some(&alice.token),
            Some(SetAvatar {
                data: Some(B64(c::encrypt_avatar(&ak, "alice", picture))),
                name: Some(B64(
                    c::encrypt_display_name(&ak, "alice", display_name).unwrap()
                )),
                details: Some(B64(
                    c::encrypt_person_details(&ak, "alice", &pronouns).unwrap()
                )),
                enc_key: B64(c::encrypt_private_data(
                    &alice.mk,
                    &alice_id,
                    "avatar-key",
                    ak.as_bytes(),
                )),
            }),
        )
        .await;
    assert_eq!(r.status, StatusCode::NO_CONTENT, "{r:?}");
    let mine: MyAvatar = h.get("/api/me/avatar", &alice.token).await.json();
    let key = c::decrypt_private_data(&alice.mk, &alice_id, "avatar-key", &mine.enc_key.unwrap())
        .unwrap();
    assert_eq!(key, ak.as_bytes());

    // No grant yet: nobody else gets it.
    let none: Option<UserAvatar> = h.get("/api/users/alice/avatar", &bob.token).await.json();
    assert!(none.is_none());
    let bob_pk: UserPublicKey = h
        .get("/api/users/bob/public-key", &alice.token)
        .await
        .json();
    let grant = AvatarGrant {
        sealed_key: B64(c::seal_avatar_key(&sealing_key(&bob_pk), &ak, "alice", "bob").unwrap()),
    };
    // Only between people who share with each other.
    let r = h
        .call(
            Method::PUT,
            "/api/avatar-grants/bob",
            Some(&alice.token),
            Some(&grant),
        )
        .await;
    assert_eq!(r.status, StatusCode::FORBIDDEN);
    let (folder, folder_key) = alice.mkdir(&h, &alice.root, "Shared").await;
    h.call(
        Method::POST,
        "/api/shares",
        Some(&alice.token),
        Some(CreateShareRequest {
            node_id: folder.clone(),
            recipient: "bob".into(),
            wrapped_key: B64(
                c::seal_share_key(&sealing_key(&bob_pk), &folder_key, &folder).unwrap(),
            ),
            permission: Permission::Read,
            expires_at: None,
        }),
    )
    .await;
    let r = h
        .call(
            Method::PUT,
            "/api/avatar-grants/bob",
            Some(&alice.token),
            Some(&grant),
        )
        .await;
    assert_eq!(r.status, StatusCode::NO_CONTENT);
    let got: UserAvatar = h
        .get("/api/users/alice/avatar", &bob.token)
        .await
        .json::<Option<UserAvatar>>()
        .unwrap();
    let k = c::open_avatar_key(&bob.kp, &got.sealed_key, "alice", "bob").unwrap();
    assert_eq!(
        c::decrypt_avatar(&k, "alice", &got.data.unwrap()).unwrap(),
        picture
    );
    assert_eq!(
        c::decrypt_display_name(&k, "alice", &got.name.unwrap()).unwrap(),
        display_name
    );
    assert_eq!(
        c::decrypt_person_details(&k, "alice", &got.details.unwrap()).unwrap(),
        pronouns
    );
    let none: Option<UserAvatar> = h.get("/api/users/alice/avatar", &carol.token).await.json();
    assert!(none.is_none());
    let mine: MyAvatar = h.get("/api/me/avatar", &alice.token).await.json();
    assert_eq!(mine.grantees, ["bob"]);

    // Once they share nothing, the grant goes.
    let s: Vec<OutgoingShare> = h.get("/api/shares/outgoing", &alice.token).await.json();
    let r = h
        .call(
            Method::DELETE,
            &format!("/api/shares/{}", s[0].id),
            Some(&alice.token),
            None::<()>,
        )
        .await;
    assert_eq!(r.status, StatusCode::NO_CONTENT);
    let none: Option<UserAvatar> = h.get("/api/users/alice/avatar", &bob.token).await.json();
    assert!(none.is_none());
    let mine: MyAvatar = h.get("/api/me/avatar", &alice.token).await.json();
    assert!(mine.grantees.is_empty());

    // Opaque to the server.
    let mut files = Vec::new();
    all_files(&h.dir.path().join("data"), &mut files);
    for p in &files {
        let bytes = std::fs::read(p).unwrap();
        for n in [
            picture.as_slice(),
            display_name.as_bytes(),
            b"zesecret",
            b"zirsecret",
            b"feminine",
            ak.as_bytes(),
        ] {
            assert!(!contains(&bytes, n), "avatar data found in {}", p.display());
        }
    }

    // Removing it takes back every grant.
    let r = h
        .call(
            Method::DELETE,
            "/api/me/avatar",
            Some(&alice.token),
            None::<()>,
        )
        .await;
    assert_eq!(r.status, StatusCode::NO_CONTENT);
    let none: Option<UserAvatar> = h.get("/api/users/alice/avatar", &bob.token).await.json();
    assert!(none.is_none());
    let mine: MyAvatar = h.get("/api/me/avatar", &alice.token).await.json();
    assert!(
        mine.data.is_none()
            && mine.name.is_none()
            && mine.details.is_none()
            && mine.grantees.is_empty()
    );
}

#[tokio::test]
async fn duplicate_names_are_refused_by_name_tag() {
    let h = Harness::new().await;
    let alice = register(&h, "alice", "pw").await;
    let root_key = alice.key_of(&h, &alice.root).await;
    let (docs, _) = alice.mkdir(&h, &alice.root, "Docs").await;

    // Same name (in any case) in the same folder: refused.
    let id = c::new_id();
    let k = Key::generate();
    let folder = |name: &str| CreateFolderRequest {
        id: id.clone(),
        parent_id: alice.root.clone(),
        enc_key: B64(c::wrap_node_key(&root_key, &k, &id)),
        enc_metadata: B64(c::encrypt_metadata(&k, &id, &meta(name, 0)).unwrap()),
        name_tag: Some(B64(c::name_tag(&root_key, name))),
    };
    let r = h
        .call(
            Method::POST,
            "/api/nodes/folder",
            Some(&alice.token),
            Some(folder("docs")),
        )
        .await;
    assert_eq!(r.status, StatusCode::CONFLICT);
    assert_eq!(r.error(), "name_taken");
    let file = alice
        .upload(&h, &alice.root, None, "a.txt", b"one")
        .await
        .unwrap();
    let err = alice
        .upload(&h, &alice.root, None, "A.TXT", b"two")
        .await
        .unwrap_err();
    assert_eq!(
        err.error(),
        "name_taken",
        "refused before any chunk is sent"
    );
    // Finishing can rename, for a name taken while the chunks went up.
    let (id, k) = (c::new_id(), Key::generate());
    let vid = c::new_id();
    let ck = Key::generate();
    let chunks = c::encrypt_content(&ck, &vid, b"late");
    let up: UploadResponse = h
        .call(
            Method::POST,
            "/api/uploads",
            Some(&alice.token),
            Some(CreateUploadRequest {
                node_id: id.clone(),
                parent_id: Some(alice.root.clone()),
                enc_key: Some(B64(c::wrap_node_key(&root_key, &k, &id))),
                enc_metadata: B64(c::encrypt_metadata(&k, &id, &meta("late.txt", 4)).unwrap()),
                version_id: vid.clone(),
                enc_content_key: B64(c::wrap_content_key(&k, &ck, &id, &vid)),
                chunk_count: 1,
                if_revision: None,
                name_tag: Some(B64(c::name_tag(&root_key, "late.txt"))),
            }),
        )
        .await
        .json();
    alice
        .upload(&h, &alice.root, None, "late.txt", b"first")
        .await
        .unwrap();
    h.raw(
        Method::PUT,
        &format!("/api/uploads/{}/chunks/0", up.upload_id),
        Some(&alice.token),
        &[],
        Body::from(chunks[0].clone()),
        Some("application/octet-stream"),
    )
    .await;
    let finish = format!("/api/uploads/{}/finish", up.upload_id);
    let r = h
        .call(Method::POST, &finish, Some(&alice.token), None::<()>)
        .await;
    assert_eq!(r.error(), "name_taken");
    let r = h
        .call(
            Method::POST,
            &finish,
            Some(&alice.token),
            Some(FinishUploadRequest {
                enc_metadata: Some(B64(
                    c::encrypt_metadata(&k, &id, &meta("late (2).txt", 4)).unwrap()
                )),
                name_tag: Some(B64(c::name_tag(&root_key, "late (2).txt"))),
            }),
        )
        .await;
    assert_eq!(r.status, StatusCode::OK, "{r:?}");
    // New versions aren't new names.
    alice
        .upload(&h, "", Some(&file), "a.txt", b"three")
        .await
        .unwrap();

    // Renaming onto a taken name is refused.
    let fk = alice.key_of(&h, &file.id).await;
    let file: Node = h
        .get(&format!("/api/nodes/{}", file.id), &alice.token)
        .await
        .json();
    let rename = |name: &str| UpdateNodeRequest {
        enc_metadata: Some(B64(
            c::encrypt_metadata(&fk, &file.id, &meta(name, 5)).unwrap()
        )),
        name_tag: Some(B64(c::name_tag(&root_key, name))),
        if_revision: Some(file.revision),
        ..Default::default()
    };
    let uri = format!("/api/nodes/{}", file.id);
    let r = h
        .call(
            Method::PATCH,
            &uri,
            Some(&alice.token),
            Some(rename("Docs")),
        )
        .await;
    assert_eq!(r.error(), "name_taken");
    let r = h
        .call(
            Method::PATCH,
            &uri,
            Some(&alice.token),
            Some(rename("b.txt")),
        )
        .await;
    assert_eq!(r.status, StatusCode::OK);

    // A trashed item frees its name; restoring it then needs a new one.
    assert_eq!(alice.delete(&h, &docs).await, StatusCode::NO_CONTENT);
    let r = h
        .call(
            Method::POST,
            "/api/nodes/folder",
            Some(&alice.token),
            Some(folder("Docs")),
        )
        .await;
    assert_eq!(r.status, StatusCode::CREATED);
    let restore = format!("/api/trash/{docs}/restore");
    let r = h
        .call(
            Method::POST,
            &restore,
            Some(&alice.token),
            Some(RestoreTrashRequest::default()),
        )
        .await;
    assert_eq!(r.error(), "name_taken");
    let dk = alice.trash(&h).await[0].path.last().unwrap().clone();
    let docs_key = c::unwrap_node_key(&root_key, &dk.enc_key, &docs).unwrap();
    let r = h
        .call(
            Method::POST,
            &restore,
            Some(&alice.token),
            Some(RestoreTrashRequest {
                enc_metadata: Some(B64(c::encrypt_metadata(
                    &docs_key,
                    &docs,
                    &meta("Docs (2)", 0),
                )
                .unwrap())),
                name_tag: Some(B64(c::name_tag(&root_key, "Docs (2)"))),
                ..Default::default()
            }),
        )
        .await;
    assert_eq!(r.status, StatusCode::OK, "{r:?}");

    // Older, untagged items can be tagged later; a clash is skipped.
    let old = c::new_id();
    let ok = Key::generate();
    sqlx::query(
        "INSERT INTO nodes (id, owner_id, created_by, parent_id, kind, enc_key, enc_metadata, created_at, updated_at) \
         SELECT ?, owner_id, owner_id, id, 'folder', ?, ?, 0, 0 FROM nodes WHERE id = ?",
    )
    .bind(&old)
    .bind(c::wrap_node_key(&root_key, &ok, &old))
    .bind(c::encrypt_metadata(&ok, &old, &meta("Old", 0)).unwrap())
    .bind(&alice.root)
    .execute(&h.state.db)
    .await
    .unwrap();
    let kids: Vec<Node> = alice.children(&h, &alice.root).await.json();
    assert!(!kids.iter().find(|n| n.id == old).unwrap().name_tagged);
    let r = h
        .call(
            Method::POST,
            &format!("/api/nodes/{}/name-tags", alice.root),
            Some(&alice.token),
            Some(NameTags {
                tags: vec![NameTagEntry {
                    id: old.clone(),
                    name_tag: B64(c::name_tag(&root_key, "Old")),
                }],
            }),
        )
        .await;
    assert_eq!(r.status, StatusCode::NO_CONTENT);
    let kids: Vec<Node> = alice.children(&h, &alice.root).await.json();
    assert!(kids.iter().all(|n| n.name_tagged));
}

#[tokio::test]
async fn drop_visitors_cannot_prune_the_owners_versions() {
    let h = Harness::with_config(|c| c.default_quota = 4000).await;
    let alice = register(&h, "alice", "pw").await;
    let (inbox, _) = alice.mkdir(&h, &alice.root, "Inbox").await;
    let f = alice
        .upload(&h, &alice.root, None, "doc.bin", &[1u8; 1000])
        .await
        .unwrap();
    let f = alice
        .upload(&h, "", Some(&f), "doc.bin", &[2u8; 1000])
        .await
        .unwrap();
    let link: Link = h
        .call(
            Method::POST,
            "/api/links",
            Some(&alice.token),
            Some(CreateLinkRequest {
                node_id: inbox.clone(),
                password_auth: None,
                enc_link_key: None,
                enc_link_secret: None,
                expires_at: None,
                upload_only: true,
                max_opens: None,
            }),
        )
        .await
        .json();
    // Only fits if an old version went; a visitor may not cause that.
    let (_, _, st) = drop_file(
        &h,
        &link.token,
        &alice.kp.public,
        &inbox,
        "big",
        &[3u8; 1900],
    )
    .await;
    assert_eq!(st, StatusCode::INSUFFICIENT_STORAGE);
    assert_eq!(alice.versions(&h, &f.id).await.len(), 2);
}

#[tokio::test]
async fn janitor_thins_old_versions_by_age() {
    let h = Harness::new().await;
    let a = register(&h, "alice", "pw").await;
    let mut f = a.upload(&h, &a.root, None, "log.txt", b"v0").await.unwrap();
    for i in 1..5 {
        f = a
            .upload(&h, "", Some(&f), "log.txt", format!("v{i}").as_bytes())
            .await
            .unwrap();
    }
    // Backdate the four old versions: two in the same day a week ago, two
    // from the last few minutes.
    let old: Vec<FileVersion> = a
        .versions(&h, &f.id)
        .await
        .into_iter()
        .filter(|v| !v.current)
        .collect();
    let day = 24 * 3600;
    let base = (thencloud_server::util::now() - 7 * day).div_euclid(day) * day;
    for (v, at) in old.iter().zip([0, 0, base + 10, base + 20]) {
        if at > 0 {
            sqlx::query("UPDATE file_versions SET created_at = ? WHERE id = ?")
                .bind(at)
                .bind(&v.id)
                .execute(&h.state.db)
                .await
                .unwrap();
        }
    }
    let used = a.me(&h).await.used_bytes;
    thencloud_server::janitor::run_once(&h.state).await.unwrap();
    let vs = a.versions(&h, &f.id).await;
    assert_eq!(vs.len(), 4, "one of the two from the same day is dropped");
    assert!(vs.iter().any(|v| v.id == old[3].id), "the newer one stays");
    assert!(!vs.iter().any(|v| v.id == old[2].id));
    assert!(!blob_exists(&h, &old[2].id));
    assert_eq!(a.me(&h).await.used_bytes, used - old[2].size);
}

#[tokio::test]
async fn full_quota_prunes_old_versions_first() {
    // Each 1000-byte upload is padded to 1024 bytes, 1064 of ciphertext.
    let h = Harness::with_config(|c| c.default_quota = 3000).await;
    let a = register(&h, "alice", "pw").await;
    let f = a
        .upload(&h, &a.root, None, "big.bin", &[1u8; 1000])
        .await
        .unwrap();
    let f = a
        .upload(&h, "", Some(&f), "big.bin", &[2u8; 1000])
        .await
        .unwrap();
    let before = a.versions(&h, &f.id).await;
    assert_eq!(before.len(), 2);
    // A third version doesn't fit until the oldest one is pruned.
    let f = a
        .upload(&h, "", Some(&f), "big.bin", &[3u8; 1000])
        .await
        .unwrap();
    let after = a.versions(&h, &f.id).await;
    assert_eq!(after.len(), 2);
    assert!(
        !after.iter().any(|v| v.id == before[1].id),
        "oldest version pruned"
    );
    assert!(a.me(&h).await.used_bytes <= 3000);
    // Current files are never pruned: a second file that can't fit fails.
    let err = a
        .upload(&h, &a.root, None, "other.bin", &[4u8; 2000])
        .await
        .unwrap_err();
    assert_eq!(err.status, StatusCode::INSUFFICIENT_STORAGE);
    assert_eq!(
        a.download(&h, &f, &a.key_of(&h, &f.id).await).await.1,
        vec![3u8; 1000]
    );
}

#[tokio::test]
async fn trash_hides_restores_and_purges() {
    let h = Harness::new().await;
    let alice = register(&h, "alice", "pw").await;
    let bob = register(&h, "bob", "pw").await;
    let (folder, folder_key) = alice.mkdir(&h, &alice.root, "Photos").await;
    let file = alice
        .upload(&h, &folder, None, "cat.jpg", b"meow")
        .await
        .unwrap();
    let file_key = alice.key_of(&h, &file.id).await;

    // Share the folder with bob (write) and make a public link to it.
    let pk: UserPublicKey = h
        .get("/api/users/bob/public-key", &alice.token)
        .await
        .json();
    h.call(
        Method::POST,
        "/api/shares",
        Some(&alice.token),
        Some(CreateShareRequest {
            node_id: folder.clone(),
            recipient: "bob".into(),
            wrapped_key: B64(c::seal_share_key(&sealing_key(&pk), &folder_key, &folder).unwrap()),
            permission: Permission::Write,
            expires_at: None,
        }),
    )
    .await;
    let link: Link = h
        .call(
            Method::POST,
            "/api/links",
            Some(&alice.token),
            Some(CreateLinkRequest {
                node_id: folder.clone(),
                password_auth: None,
                enc_link_key: None,
                enc_link_secret: None,
                expires_at: None,
                upload_only: false,
                max_opens: None,
            }),
        )
        .await
        .json();
    let public = format!("/api/public/{}", link.token);

    // Bob deletes the file inside the shared folder: it lands in *alice's* trash.
    assert_eq!(bob.delete(&h, &file.id).await, StatusCode::NO_CONTENT);
    assert!(bob.trash(&h).await.is_empty());
    let t = alice.trash(&h).await;
    assert_eq!(t.len(), 1);
    assert_eq!(t[0].trashed_by, "bob");
    assert!(
        alice
            .children(&h, &folder)
            .await
            .json::<Vec<Node>>()
            .is_empty()
    );

    // The trash listing carries the path from the root, so the client can
    // unwrap the key and show the name and where it was.
    let ids: Vec<&str> = t[0].path.iter().map(|n| n.id.as_str()).collect();
    assert_eq!(
        ids,
        vec![alice.root.as_str(), folder.as_str(), file.id.as_str()]
    );
    let mut k = c::unwrap_node_key(&alice.mk, &t[0].path[0].enc_key, &t[0].path[0].id).unwrap();
    for n in &t[0].path[1..] {
        k = c::unwrap_node_key(&k, &n.enc_key, &n.id).unwrap();
    }
    assert_eq!(
        c::decrypt_metadata(&k, &file.id, &t[0].node.enc_metadata)
            .unwrap()
            .name,
        "cat.jpg"
    );

    // Restore puts it back where it was, still decryptable.
    let r = h
        .call(
            Method::POST,
            &format!("/api/trash/{}/restore", file.id),
            Some(&alice.token),
            Some(RestoreTrashRequest::default()),
        )
        .await;
    assert_eq!(r.status, StatusCode::OK, "{r:?}");
    let restored: Node = r.json();
    assert_eq!(alice.download(&h, &restored, &file_key).await.1, b"meow");

    // Trashing the folder hides the whole subtree from everyone.
    assert_eq!(alice.delete(&h, &folder).await, StatusCode::NO_CONTENT);
    assert_eq!(
        alice.children(&h, &folder).await.status,
        StatusCode::NOT_FOUND
    );
    let r = h
        .get(&format!("/api/nodes/{}/chunks/0", file.id), &alice.token)
        .await;
    assert_eq!(r.status, StatusCode::NOT_FOUND);
    assert_eq!(
        bob.children(&h, &folder).await.status,
        StatusCode::NOT_FOUND
    );
    assert!(
        h.get("/api/shares/incoming", &bob.token)
            .await
            .json::<Vec<IncomingShare>>()
            .is_empty()
    );
    assert!(
        h.get("/api/links", &alice.token)
            .await
            .json::<Vec<Link>>()
            .is_empty()
    );
    let r = h
        .raw(Method::GET, &public, None, &[], Body::empty(), None)
        .await;
    assert_eq!(r.status, StatusCode::NOT_FOUND);
    // Nothing can be created in a trashed folder.
    let id = c::new_id();
    let r = h
        .call(
            Method::POST,
            "/api/nodes/folder",
            Some(&alice.token),
            Some(CreateFolderRequest {
                id: id.clone(),
                parent_id: folder.clone(),
                enc_key: B64(c::wrap_node_key(&folder_key, &Key::generate(), &id)),
                enc_metadata: B64(
                    c::encrypt_metadata(&Key::generate(), &id, &meta("x", 0)).unwrap()
                ),
                name_tag: None,
            }),
        )
        .await;
    assert_eq!(r.status, StatusCode::NOT_FOUND);

    // Trash the file and then its folder, and restore the file on its own:
    // its folder is in the trash, so the client must pick another folder and
    // re-wrap the file's key for it.
    let r = h
        .call(
            Method::POST,
            &format!("/api/trash/{folder}/restore"),
            Some(&alice.token),
            Some(RestoreTrashRequest::default()),
        )
        .await;
    assert_eq!(r.status, StatusCode::OK);
    assert_eq!(alice.delete(&h, &file.id).await, StatusCode::NO_CONTENT);
    assert_eq!(alice.delete(&h, &folder).await, StatusCode::NO_CONTENT);
    assert_eq!(alice.trash(&h).await.len(), 2);
    let r = h
        .call(
            Method::POST,
            &format!("/api/trash/{}/restore", file.id),
            Some(&alice.token),
            Some(RestoreTrashRequest::default()),
        )
        .await;
    assert_eq!(r.status, StatusCode::CONFLICT);
    assert_eq!(r.error(), "parent_unavailable");
    let root_key = alice.key_of(&h, &alice.root).await;
    let r = h
        .call(
            Method::POST,
            &format!("/api/trash/{}/restore", file.id),
            Some(&alice.token),
            Some(RestoreTrashRequest {
                parent_id: Some(alice.root.clone()),
                enc_key: Some(B64(c::wrap_node_key(&root_key, &file_key, &file.id))),
                ..Default::default()
            }),
        )
        .await;
    assert_eq!(r.status, StatusCode::OK, "{r:?}");
    let kids: Vec<Node> = alice.children(&h, &alice.root).await.json();
    assert_eq!(names(&kids, &root_key), vec!["cat.jpg"]);

    // Other users can't see or touch alice's trash.
    let r = h
        .call(
            Method::DELETE,
            &format!("/api/trash/{folder}"),
            Some(&bob.token),
            None::<()>,
        )
        .await;
    assert_eq!(r.status, StatusCode::NOT_FOUND);

    // Purging one item frees its space; the janitor purges expired items.
    let r = h
        .call(
            Method::DELETE,
            &format!("/api/trash/{folder}"),
            Some(&alice.token),
            None::<()>,
        )
        .await;
    assert_eq!(r.status, StatusCode::NO_CONTENT);
    assert!(alice.trash(&h).await.is_empty());
    assert_eq!(alice.delete(&h, &file.id).await, StatusCode::NO_CONTENT);
    assert_eq!(
        thencloud_server::routes::trash::purge_expired(&h.state, 0)
            .await
            .unwrap(),
        1
    );
    assert!(alice.trash(&h).await.is_empty());
    assert_eq!(alice.me(&h).await.used_bytes, 0);
}

#[tokio::test]
async fn sessions_list_and_revoke() {
    let h = Harness::new().await;
    let laptop = register(&h, "dave", "dave's password").await;
    let phone = login(&h, "dave", "dave's password").await.ok().unwrap();
    let other = register(&h, "erin", "erin's password").await;

    let r = h.get("/api/sessions", &laptop.token).await;
    assert_eq!(r.status, StatusCode::OK);
    let text = String::from_utf8(r.body.clone()).unwrap();
    assert!(
        !text.contains("token"),
        "no token material in the list: {text}"
    );
    let list: Vec<DeviceSession> = r.json();
    assert_eq!(list.len(), 2);
    assert_eq!(list.iter().filter(|s| s.current).count(), 1);
    let phone_id = {
        let from_phone: Vec<DeviceSession> = h.get("/api/sessions", &phone.token).await.json();
        from_phone.into_iter().find(|s| s.current).unwrap().id
    };

    // Someone else can't sign your devices out.
    let r = h
        .call(
            Method::DELETE,
            &format!("/api/sessions/{phone_id}"),
            Some(&other.token),
            None::<()>,
        )
        .await;
    assert_eq!(r.status, StatusCode::NOT_FOUND);
    assert_eq!(h.get("/api/me", &phone.token).await.status, StatusCode::OK);

    // Signing the phone out from the laptop ends its session.
    let r = h
        .call(
            Method::DELETE,
            &format!("/api/sessions/{phone_id}"),
            Some(&laptop.token),
            None::<()>,
        )
        .await;
    assert_eq!(r.status, StatusCode::NO_CONTENT);
    assert_eq!(
        h.get("/api/me", &phone.token).await.status,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(h.get("/api/me", &laptop.token).await.status, StatusCode::OK);

    // "Sign out everywhere else" keeps only the current session.
    let tablet = login(&h, "dave", "dave's password").await.ok().unwrap();
    let r = h
        .call(
            Method::DELETE,
            "/api/sessions",
            Some(&laptop.token),
            None::<()>,
        )
        .await;
    assert_eq!(r.status, StatusCode::NO_CONTENT);
    assert_eq!(
        h.get("/api/me", &tablet.token).await.status,
        StatusCode::UNAUTHORIZED
    );
    let list: Vec<DeviceSession> = h.get("/api/sessions", &laptop.token).await.json();
    assert_eq!(list.len(), 1);
    assert!(list[0].current);
    // Other users' sessions are untouched.
    assert_eq!(h.get("/api/me", &other.token).await.status, StatusCode::OK);
}

#[tokio::test]
async fn admin_users_registration_and_invites() {
    let h = Harness::new().await;
    let admin = register(&h, "root", "admin password").await;
    let bob = register(&h, "bob", "bob's password").await;
    let del = |uri: String, t: &str| {
        let uri = uri.clone();
        let t = t.to_string();
        let h = &h;
        async move { h.call(Method::DELETE, &uri, Some(&t), None::<()>).await }
    };

    // Only admins get in.
    for uri in [
        "/api/admin/users",
        "/api/admin/stats",
        "/api/admin/settings",
        "/api/admin/invites",
    ] {
        assert_eq!(
            h.get(uri, &bob.token).await.status,
            StatusCode::FORBIDDEN,
            "{uri}"
        );
    }
    let users: Vec<AdminUser> = h.get("/api/admin/users", &admin.token).await.json();
    assert_eq!(users.len(), 2);
    let bob_id = users
        .iter()
        .find(|u| u.username == "bob")
        .unwrap()
        .id
        .clone();
    let root_id = users
        .iter()
        .find(|u| u.username == "root")
        .unwrap()
        .id
        .clone();
    assert!(
        users
            .iter()
            .find(|u| u.username == "root")
            .unwrap()
            .is_admin
    );

    // Invite-only registration.
    let r = h
        .call(
            Method::PATCH,
            "/api/admin/settings",
            Some(&admin.token),
            Some(json!({"registration": "invite"})),
        )
        .await;
    assert_eq!(r.status, StatusCode::OK);
    let opts: AuthOptions = h
        .call(Method::GET, "/api/auth/options", None, None::<()>)
        .await
        .json();
    assert_eq!(opts.registration, Registration::Invite);
    let err = try_register(&h, "carol", "carol's password", None)
        .await
        .err()
        .unwrap();
    assert_eq!(err.status, StatusCode::FORBIDDEN);
    let r = h
        .call(
            Method::POST,
            "/api/admin/invites",
            Some(&admin.token),
            Some(json!({"days": 0})),
        )
        .await;
    assert_eq!(r.status, StatusCode::BAD_REQUEST);
    let inv: CreatedInvite = h
        .call(
            Method::POST,
            "/api/admin/invites",
            Some(&admin.token),
            Some(json!({"days": 7})),
        )
        .await
        .json();
    let bad = try_register(&h, "carol", "carol's password", Some("not-a-real-invite"))
        .await
        .err()
        .unwrap();
    assert_eq!(bad.status, StatusCode::FORBIDDEN);
    try_register(&h, "carol", "carol's password", Some(&inv.token))
        .await
        .ok()
        .unwrap();
    // Single use.
    let again = try_register(&h, "dan", "dan's password", Some(&inv.token))
        .await
        .err()
        .unwrap();
    assert_eq!(again.status, StatusCode::FORBIDDEN);
    let list: Vec<Invite> = h.get("/api/admin/invites", &admin.token).await.json();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].used_by.as_deref(), Some("carol"));
    assert!(
        !String::from_utf8(h.get("/api/admin/invites", &admin.token).await.body)
            .unwrap()
            .contains(&inv.token)
    );

    // Closed: even a fresh invite doesn't work.
    let inv2: CreatedInvite = h
        .call(
            Method::POST,
            "/api/admin/invites",
            Some(&admin.token),
            Some(json!({"days": 1})),
        )
        .await
        .json();
    h.call(
        Method::PATCH,
        "/api/admin/settings",
        Some(&admin.token),
        Some(json!({"registration": "closed"})),
    )
    .await;
    let closed = try_register(&h, "dan", "dan's password", Some(&inv2.token))
        .await
        .err()
        .unwrap();
    assert_eq!(closed.status, StatusCode::FORBIDDEN);
    assert_eq!(
        del(
            format!("/api/admin/invites/{}", inv2.invite.id),
            &admin.token
        )
        .await
        .status,
        StatusCode::NO_CONTENT
    );

    // Quotas, and disabling.
    let file = bob
        .upload(&h, &bob.root, None, "bob.txt", b"hello from bob")
        .await
        .unwrap();
    let vid = file.version.as_ref().unwrap().id.clone();
    assert!(blob_exists(&h, &vid));
    let u: AdminUser = h
        .call(
            Method::PATCH,
            &format!("/api/admin/users/{bob_id}"),
            Some(&admin.token),
            Some(json!({"quota_bytes": 1000})),
        )
        .await
        .json();
    assert_eq!(u.quota_bytes, 1000);
    assert!(u.used_bytes > 0);
    let r = h
        .call(
            Method::PATCH,
            &format!("/api/admin/users/{bob_id}"),
            Some(&admin.token),
            Some(json!({"disabled": true})),
        )
        .await;
    assert_eq!(r.status, StatusCode::OK);
    assert_eq!(
        h.get("/api/me", &bob.token).await.status,
        StatusCode::UNAUTHORIZED
    );
    let err = login(&h, "bob", "bob's password").await.err().unwrap();
    assert_eq!(err.status, StatusCode::FORBIDDEN);
    assert!(String::from_utf8_lossy(&err.body).contains("account_disabled"));
    // A wrong password still just looks wrong.
    assert_eq!(
        login(&h, "bob", "nope").await.err().unwrap().status,
        StatusCode::UNAUTHORIZED
    );
    h.call(
        Method::PATCH,
        &format!("/api/admin/users/{bob_id}"),
        Some(&admin.token),
        Some(json!({"disabled": false})),
    )
    .await;
    let bob = login(&h, "bob", "bob's password").await.ok().unwrap();

    // Admins can't lock themselves out.
    for body in [json!({"disabled": true}), json!({"is_admin": false})] {
        let r = h
            .call(
                Method::PATCH,
                &format!("/api/admin/users/{root_id}"),
                Some(&admin.token),
                Some(body),
            )
            .await;
        assert_eq!(r.status, StatusCode::BAD_REQUEST);
    }
    assert_eq!(
        del(format!("/api/admin/users/{root_id}"), &admin.token)
            .await
            .status,
        StatusCode::BAD_REQUEST
    );

    let stats: ServerStats = h.get("/api/admin/stats", &admin.token).await.json();
    assert_eq!(stats.users, 3);
    assert_eq!(stats.files, 1);

    // Deleting an account removes its data and blobs.
    assert_eq!(
        del(format!("/api/admin/users/{bob_id}"), &admin.token)
            .await
            .status,
        StatusCode::NO_CONTENT
    );
    assert!(!blob_exists(&h, &vid));
    assert_eq!(
        h.get("/api/me", &bob.token).await.status,
        StatusCode::UNAUTHORIZED
    );
    let users: Vec<AdminUser> = h.get("/api/admin/users", &admin.token).await.json();
    assert_eq!(users.len(), 2);
    let stats: ServerStats = h.get("/api/admin/stats", &admin.token).await.json();
    assert_eq!((stats.files, stats.users), (0, 2));
}

#[tokio::test]
async fn recovery_key_resets_a_forgotten_password() {
    let h = Harness::new().await;
    let gina = register(&h, "gina", "gina's old password").await;
    // The current password's auth key, as the client would derive it.
    let auth_for = |password: &str| {
        let h = &h;
        let password = password.to_string();
        async move {
            let pre: PreloginResponse = h
                .call(
                    Method::POST,
                    "/api/auth/prelogin",
                    None,
                    Some(json!({"username": "gina"})),
                )
                .await
                .json();
            let ak = c::derive_account_keys(&password, &pre.kdf_salt, pre.kdf_params).unwrap();
            B64(ak.auth_key.as_bytes().to_vec())
        }
    };

    // Set up a recovery key (needs the current password).
    let rk = Key::generate();
    let rk_text = c::encode_recovery_key(&rk);
    let derived = c::derive_recovery_keys(&rk);
    let set = |auth: B64| SetRecoveryRequest {
        current_auth_key: auth,
        recovery_auth_key: B64(derived.auth_key.as_bytes().to_vec()),
        enc_master_key_recovery: B64(c::wrap_master_key_recovery(&derived.kek, &gina.mk)),
    };
    let r = h
        .call(
            Method::POST,
            "/api/auth/recovery",
            Some(&gina.token),
            Some(set(auth_for("wrong").await)),
        )
        .await;
    assert_eq!(r.status, StatusCode::UNAUTHORIZED);
    let me: Me = h
        .call(
            Method::POST,
            "/api/auth/recovery",
            Some(&gina.token),
            Some(set(auth_for("gina's old password").await)),
        )
        .await
        .json();
    assert!(me.recovery_created_at.is_some());

    // Unlock: wrong keys and unknown users look the same.
    let unlock = |user: &str, auth: &[u8]| {
        let body = json!({"username": user, "recovery_auth_key": B64(auth.to_vec())});
        let h = &h;
        async move {
            h.call(Method::POST, "/api/auth/recovery/unlock", None, Some(body))
                .await
        }
    };
    let wrong = c::derive_recovery_keys(&Key::generate());
    let r1 = unlock("gina", wrong.auth_key.as_bytes()).await;
    let r2 = unlock("nobody", derived.auth_key.as_bytes()).await;
    assert_eq!(r1.status, StatusCode::UNAUTHORIZED);
    assert_eq!((r2.status, r2.body.clone()), (r1.status, r1.body.clone()));
    let r: RecoveryUnlockResponse = unlock("gina", derived.auth_key.as_bytes()).await.json();
    let typed = c::decode_recovery_key(&rk_text.to_lowercase()).unwrap();
    let mk = c::unwrap_master_key_recovery(
        &c::derive_recovery_keys(&typed).kek,
        &r.enc_master_key_recovery.0,
    )
    .unwrap();
    assert!(mk == gina.mk);

    // Reset to a new password; old sessions end, the master key is the same.
    let salt = c::random_bytes(c::SALT_LEN);
    let ak = c::derive_account_keys("gina's new password", &salt, FAST_KDF).unwrap();
    let reset = RecoveryResetRequest {
        username: "gina".into(),
        recovery_auth_key: B64(derived.auth_key.as_bytes().to_vec()),
        new_auth_key: B64(ak.auth_key.as_bytes().to_vec()),
        new_kdf_salt: B64(salt),
        new_kdf_params: FAST_KDF,
        new_enc_master_key: B64(c::wrap_master_key(&ak.kek, &mk)),
        device_name: None,
    };
    let s: SessionResponse = h
        .call(Method::POST, "/api/auth/recovery/reset", None, Some(&reset))
        .await
        .json();
    assert_eq!(
        h.get("/api/me", &gina.token).await.status,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(h.get("/api/me", &s.token).await.status, StatusCode::OK);
    assert!(login(&h, "gina", "gina's old password").await.is_err());
    let again = login(&h, "gina", "gina's new password").await.ok().unwrap();
    assert!(again.mk == gina.mk);

    // Nothing that could unwrap the master key is stored.
    let mut files = Vec::new();
    all_files(&h.dir.path().join("data"), &mut files);
    let needles: Vec<&[u8]> = vec![
        rk.as_bytes(),
        derived.kek.as_bytes(),
        derived.auth_key.as_bytes(),
        rk_text.as_bytes(),
        gina.mk.as_bytes(),
        b"gina's old password",
        b"gina's new password",
    ];
    for p in &files {
        let bytes = std::fs::read(p).unwrap();
        for n in &needles {
            assert!(!contains(&bytes, n), "secret found in {}", p.display());
        }
    }

    // The recovery key keeps working until it's removed (with the password).
    assert_eq!(
        unlock("gina", derived.auth_key.as_bytes()).await.status,
        StatusCode::OK
    );
    let me: Me = h
        .call(
            Method::DELETE,
            "/api/auth/recovery",
            Some(&again.token),
            Some(json!({"current_auth_key": auth_for("gina's new password").await})),
        )
        .await
        .json();
    assert!(me.recovery_created_at.is_none());
    assert_eq!(
        unlock("gina", derived.auth_key.as_bytes()).await.status,
        StatusCode::UNAUTHORIZED
    );
}

#[tokio::test]
async fn verified_contacts_are_opaque_to_the_server() {
    let h = Harness::new().await;
    let ivy = register(&h, "ivy", "ivy's password").await;
    let me: Me = h.get("/api/me", &ivy.token).await.json();

    let empty: PrivateData = h.get("/api/me/contacts", &ivy.token).await.json();
    assert!(empty.data.is_none());
    assert_eq!(empty.revision, 0);

    let plain = br#"{"jules-marker-contact":{"public_key":"abc","verified_at":1}}"#;
    let sealed = c::encrypt_private_data(&ivy.mk, &me.user_id, "contacts", plain);
    let put = |data: Vec<u8>, rev: i64| PutPrivateData {
        data: B64(data),
        if_revision: rev,
    };
    let r = h
        .call(
            Method::PUT,
            "/api/me/contacts",
            Some(&ivy.token),
            Some(put(sealed.clone(), 0)),
        )
        .await;
    assert_eq!(r.status, StatusCode::OK);
    // A second device working from the old revision is told to reload.
    let r = h
        .call(
            Method::PUT,
            "/api/me/contacts",
            Some(&ivy.token),
            Some(put(sealed.clone(), 0)),
        )
        .await;
    assert_eq!(r.status, StatusCode::CONFLICT);

    let got: PrivateData = h.get("/api/me/contacts", &ivy.token).await.json();
    assert_eq!(got.revision, 1);
    let opened =
        c::decrypt_private_data(&ivy.mk, &me.user_id, "contacts", &got.data.unwrap().0).unwrap();
    assert_eq!(opened, plain);

    // Another user's master key and id don't open it.
    let other = register(&h, "kai", "kai's password").await;
    let other_me: Me = h.get("/api/me", &other.token).await.json();
    assert!(c::decrypt_private_data(&other.mk, &other_me.user_id, "contacts", &sealed).is_err());

    let mut files = Vec::new();
    all_files(&h.dir.path().join("data"), &mut files);
    for p in &files {
        let bytes = std::fs::read(p).unwrap();
        assert!(
            !contains(&bytes, b"jules-marker-contact"),
            "contacts plaintext in {}",
            p.display()
        );
    }
}

#[tokio::test]
async fn app_data_is_opaque_to_the_server() {
    let h = Harness::new().await;
    let lea = register(&h, "lea", "lea's password").await;
    let me: Me = h.get("/api/me", &lea.token).await.json();

    let empty: PrivateData = h.get("/api/me/data/music", &lea.token).await.json();
    assert!(empty.data.is_none());
    assert_eq!(empty.revision, 0);
    let r = h
        .call(
            Method::GET,
            "/api/me/data/other",
            Some(&lea.token),
            None::<()>,
        )
        .await;
    assert_eq!(r.status, StatusCode::NOT_FOUND);

    let plain = br#"{"playlists":[{"name":"mira-marker-playlist"}]}"#;
    let sealed = c::encrypt_private_data(&lea.mk, &me.user_id, "music", plain);
    let put = |data: Vec<u8>, rev: i64| PutPrivateData {
        data: B64(data),
        if_revision: rev,
    };
    for (rev, status) in [
        (0, StatusCode::OK),
        (0, StatusCode::CONFLICT),
        (1, StatusCode::OK),
        (1, StatusCode::CONFLICT),
    ] {
        let r = h
            .call(
                Method::PUT,
                "/api/me/data/music",
                Some(&lea.token),
                Some(put(sealed.clone(), rev)),
            )
            .await;
        assert_eq!(r.status, status);
    }

    let got: PrivateData = h.get("/api/me/data/music", &lea.token).await.json();
    assert_eq!(got.revision, 2);
    let opened =
        c::decrypt_private_data(&lea.mk, &me.user_id, "music", &got.data.unwrap().0).unwrap();
    assert_eq!(opened, plain);
    assert!(c::decrypt_private_data(&lea.mk, &me.user_id, "videos", &sealed).is_err());

    // Each user has their own.
    let other = register(&h, "max", "max's password").await;
    let theirs: PrivateData = h.get("/api/me/data/music", &other.token).await.json();
    assert!(theirs.data.is_none());

    // The health log is app data like the rest: stored, never readable.
    let health = br#"{"measures":[{"kind":"weight","value":81.5,"note":"mira-marker-weight"}]}"#;
    let r = h
        .call(
            Method::PUT,
            "/api/me/data/health",
            Some(&lea.token),
            Some(put(
                c::encrypt_private_data(&lea.mk, &me.user_id, "health", health),
                0,
            )),
        )
        .await;
    assert_eq!(r.status, StatusCode::OK);

    let mut files = Vec::new();
    all_files(&h.dir.path().join("data"), &mut files);
    for p in &files {
        let bytes = std::fs::read(p).unwrap();
        assert!(
            !contains(&bytes, b"mira-marker-weight"),
            "health data plaintext in {}",
            p.display()
        );
        assert!(
            !contains(&bytes, b"mira-marker-playlist"),
            "app data plaintext in {}",
            p.display()
        );
    }
}

/// A stand-in for yt-dlp: checks it was called with the safety flags,
/// answers lookups with fixed JSON, streams fake bytes for downloads, and
/// litters its working directory so we can check the server cleans up.
const FAKE_YT_DLP: &str = r#"#!/bin/sh
case " $* " in *" --version "*) echo 2099.01.01; exit 0;; esac
for f in --ignore-config --no-cache-dir --no-playlist --no-part "--use-extractors default,-generic"; do
  case " $* " in *" $f "*) ;; *) echo "ERROR: missing $f" >&2; exit 9;; esac
done
echo leftover > scratch-file.tmp
url=""; for a in "$@"; do url="$a"; done
case "$url" in *fail*) echo "ERROR: [youtube] abc: Video unavailable" >&2; exit 1;; esac
case " $* " in *" --dump-single-json --flat-playlist "*) ;; *" --dump-single-json "*) echo "ERROR: lookups must be flat" >&2; exit 9;; esac
case "$url" in *playlist*)
  echo '{"_type":"playlist","title":"A list","extractor_key":"YoutubeTab","entries":[{"url":"https://videos.test/watch?v=a","title":"First","duration":61},{"url":"file:///etc/passwd","title":"Nope"},{"url":"https://videos.test/watch?v=b","title":"Second"}]}'
  exit 0;;
esac
case " $* " in *" --dump-single-json "*)
  echo '{"title":"A test clip","extractor_key":"Youtube","uploader":"Someone","duration":12.5,"formats":[{"format_id":"140","ext":"m4a","vcodec":"none","acodec":"mp4a","filesize":900,"protocol":"https"},{"format_id":"134","ext":"mp4","vcodec":"avc1","acodec":"none","height":240,"filesize":2000,"protocol":"https"},{"format_id":"18","ext":"mp4","vcodec":"avc1","acodec":"mp4a","height":360,"filesize":3000,"protocol":"https"},{"format_id":"9999","ext":"mp4","vcodec":"avc1","acodec":"mp4a","height":2160,"protocol":"https"}]}'
  exit 0;;
esac
case "$url" in *big*) head -c 5000 /dev/zero; exit 0;; esac
case "$url" in *slow*) printf 'FAKE'; sleep 3; printf 'VIDEO'; exit 0;; esac
case " $* " in
  *" -f 140 "*) printf 'AUDIO-ONLY-BYTES'; exit 0;;
  *" -f 134 "*) printf 'VIDEO-ONLY-BYTES'; exit 0;;
  *" -f 9999 "*) printf 'UHD-BYTES'; exit 0;;
  *" -f 18 "*) ;;
  *) echo "ERROR: unexpected format: $*" >&2; exit 8;;
esac
i=0; while [ $i -lt 100 ]; do printf 'FAKE-VIDEO-BYTES'; i=$((i+1)); done
"#;

#[tokio::test]
async fn video_downloader_is_opt_in_streamed_and_cleaned_up() {
    use std::os::unix::fs::PermissionsExt;
    let bin = tempfile::tempdir().unwrap();
    let script = bin.path().join("yt-dlp");
    std::fs::write(&script, FAKE_YT_DLP).unwrap();
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
    let h = Harness::with_config(|c| {
        c.yt_dlp = script.clone();
        c.ffmpeg = bin.path().join("no-ffmpeg"); // single-file formats only
        c.downloader_max_bytes = 4000;
        c.downloader_public_only = false; // the fake links don't resolve
    })
    .await;
    let admin = register(&h, "root", "admin password").await;
    let user = register(&h, "mia", "mia's password").await;
    let post = |uri: &'static str, token: String, body: serde_json::Value| {
        let h = &h;
        async move { h.call(Method::POST, uri, Some(&token), Some(body)).await }
    };
    let link = |u: &str| json!({"url": u});

    // Off by default, for everyone.
    let tools: ToolsInfo = h.get("/api/tools", &admin.token).await.json();
    assert!(!tools.video_downloader);
    let r = post(
        "/api/tools/video/info",
        admin.token.clone(),
        link("https://videos.test/watch?v=1"),
    )
    .await;
    assert_eq!(r.status, StatusCode::FORBIDDEN);
    let s: AdminSettings = h.get("/api/admin/settings", &admin.token).await.json();
    assert_eq!(s.downloader, DownloaderAccess::Off);
    assert_eq!(s.yt_dlp_version.as_deref(), Some("2099.01.01"));

    // Admins only.
    h.call(
        Method::PATCH,
        "/api/admin/settings",
        Some(&admin.token),
        Some(json!({"downloader": "admins"})),
    )
    .await;
    assert!(
        h.get("/api/tools", &admin.token)
            .await
            .json::<ToolsInfo>()
            .video_downloader
    );
    assert!(
        !h.get("/api/tools", &user.token)
            .await
            .json::<ToolsInfo>()
            .video_downloader
    );
    let r = post(
        "/api/tools/video/info",
        user.token.clone(),
        link("https://videos.test/watch?v=1"),
    )
    .await;
    assert_eq!(r.status, StatusCode::FORBIDDEN);

    // Lookup.
    let info: VideoInfo = post(
        "/api/tools/video/info",
        admin.token.clone(),
        link("https://videos.test/watch?v=1"),
    )
    .await
    .json();
    assert_eq!(info.title, "A test clip");
    assert_eq!(info.site, "Youtube");
    let v = info.video.unwrap();
    assert_eq!(
        (v.ext.as_str(), v.size, v.height),
        ("mp4", Some(3000), Some(360))
    );
    assert_eq!(info.audio.unwrap().ext, "m4a");
    // Each quality that gives something different: 360p (for anything up
    // to 1080p) and the 2160p file for the best.
    let q: Vec<_> = info
        .qualities
        .iter()
        .map(|o| (o.height, o.quality))
        .collect();
    assert_eq!(
        q,
        [
            (Some(360), Some(VideoQuality::P480)),
            (Some(2160), Some(VideoQuality::Best))
        ]
    );
    assert!(info.entries.is_empty());
    // A playlist lists its videos (http(s) links only), to fetch one by one.
    let list: VideoInfo = post(
        "/api/tools/video/info",
        admin.token.clone(),
        link("https://videos.test/playlist?list=x"),
    )
    .await
    .json();
    assert!(list.video.is_none() && list.audio.is_none());
    let urls: Vec<_> = list.entries.iter().map(|e| e.url.as_str()).collect();
    assert_eq!(
        urls,
        [
            "https://videos.test/watch?v=a",
            "https://videos.test/watch?v=b"
        ]
    );
    assert_eq!(list.entries[0].title, "First");
    let r = post(
        "/api/tools/video/info",
        admin.token.clone(),
        link("https://videos.test/fail"),
    )
    .await;
    assert_eq!(r.status, StatusCode::BAD_REQUEST);
    assert!(String::from_utf8_lossy(&r.body).contains("Video unavailable"));
    for bad in [
        "ftp://videos.test/x",
        "https://user:pw@videos.test/x",
        "https://videos.test/a b",
        "videos.test/x",
    ] {
        let r = post("/api/tools/video/info", admin.token.clone(), link(bad)).await;
        assert_eq!(r.status, StatusCode::BAD_REQUEST, "{bad}");
    }

    // Download: streamed through, nothing left behind.
    h.call(
        Method::PATCH,
        "/api/admin/settings",
        Some(&admin.token),
        Some(json!({"downloader": "everyone"})),
    )
    .await;
    let r = post(
        "/api/tools/video/download",
        user.token.clone(),
        json!({"url": "https://videos.test/watch?v=1", "kind": "video"}),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK);
    assert_eq!(r.body, b"FAKE-VIDEO-BYTES".repeat(100));
    assert_eq!(r.headers.get("cache-control").unwrap(), "no-store");
    let r = post(
        "/api/tools/video/download",
        user.token.clone(),
        json!({"url": "https://videos.test/watch?v=1", "kind": "video", "quality": "best"}),
    )
    .await;
    assert_eq!(r.body, b"UHD-BYTES");

    // Over the size cap: the response is cut off with an error, not ended
    // cleanly, so a partial file can't pass for a whole one.
    let send = |url: &str, token: &str| {
        Request::builder()
            .method(Method::POST)
            .uri("/api/tools/video/download")
            .header("authorization", format!("Bearer {token}"))
            .header("content-type", "application/json")
            .body(Body::from(json!({"url": url}).to_string()))
            .unwrap()
    };
    let res = h
        .app
        .clone()
        .oneshot(send("https://videos.test/big", &user.token))
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    assert!(
        res.into_body().collect().await.is_err(),
        "oversized download must fail"
    );

    // One at a time per user: while a download is open, another is refused.
    let first = h
        .app
        .clone()
        .oneshot(send("https://videos.test/slow", &user.token))
        .await
        .unwrap();
    assert_eq!(first.status(), StatusCode::OK);
    let second = h
        .app
        .clone()
        .oneshot(send("https://videos.test/watch?v=2", &user.token))
        .await
        .unwrap();
    assert_eq!(second.status(), StatusCode::TOO_MANY_REQUESTS);
    // Someone else isn't affected.
    let other = h
        .app
        .clone()
        .oneshot(send("https://videos.test/watch?v=3", &admin.token))
        .await
        .unwrap();
    assert_eq!(other.status(), StatusCode::OK);
    drop(other);
    // Dropping the response (the browser leaving) frees the slot.
    drop(first);
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    let again = h
        .app
        .clone()
        .oneshot(send("https://videos.test/watch?v=4", &user.token))
        .await
        .unwrap();
    assert_eq!(again.status(), StatusCode::OK);
    assert_eq!(
        again.into_body().collect().await.unwrap().to_bytes().len(),
        1600
    );

    // Scratch directories are gone. They're removed as the stream ends,
    // which may be just after the last byte arrives here.
    let mut files = Vec::new();
    for _ in 0..50 {
        files.clear();
        all_files(&h.dir.path().join("data/downloads"), &mut files);
        if files.is_empty() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    assert!(files.is_empty(), "left behind: {files:?}");
}

#[tokio::test]
async fn video_downloader_refuses_private_addresses() {
    use std::os::unix::fs::PermissionsExt;
    let bin = tempfile::tempdir().unwrap();
    let script = bin.path().join("yt-dlp");
    std::fs::write(&script, FAKE_YT_DLP).unwrap();
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
    let h = Harness::with_config(|c| c.yt_dlp = script.clone()).await;
    let admin = register(&h, "root", "admin password").await;
    h.call(
        Method::PATCH,
        "/api/admin/settings",
        Some(&admin.token),
        Some(json!({"downloader": "admins"})),
    )
    .await;
    for url in [
        "http://127.0.0.1/video",
        "http://localhost:8080/x",
        "https://[::1]/x",
        "http://169.254.169.254/latest/meta-data",
    ] {
        let r = h
            .call(
                Method::POST,
                "/api/tools/video/info",
                Some(&admin.token),
                Some(json!({"url": url})),
            )
            .await;
        assert_eq!(r.status, StatusCode::BAD_REQUEST, "{url}");
    }
}

/// With ffmpeg, video goes yt-dlp -> ffmpeg -> browser. A pass-through
/// ffmpeg checks the pipeline itself: both processes, their exit statuses,
/// and that the remux arguments are what we expect.
#[tokio::test]
async fn video_downloader_pipes_video_through_ffmpeg() {
    use std::os::unix::fs::PermissionsExt;
    let bin = tempfile::tempdir().unwrap();
    let exe = |name: &str, body: &str| {
        let p = bin.path().join(name);
        std::fs::write(&p, body).unwrap();
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
        p
    };
    let yt = exe("yt-dlp", FAKE_YT_DLP);
    let ff = exe(
        "ffmpeg",
        r#"#!/bin/sh
case " $* " in *" -version "*) echo ffmpeg fake; exit 0;; esac
case " $* " in *" -c copy -f mp4 "*"pipe:1"*) ;; *) exit 7;; esac
# Read every -i input in turn, like a merge would.
printf 'MERGED'
prev=""; for a in "$@"; do [ "$prev" = "-i" ] && { printf ':'; cat "$a"; }; prev="$a"; done
"#,
    );
    let h = Harness::with_config(|c| {
        c.yt_dlp = yt.clone();
        c.ffmpeg = ff.clone();
        c.downloader_public_only = false;
    })
    .await;
    let admin = register(&h, "root", "admin password").await;
    h.call(
        Method::PATCH,
        "/api/admin/settings",
        Some(&admin.token),
        Some(json!({"downloader": "admins"})),
    )
    .await;
    let s: AdminSettings = h.get("/api/admin/settings", &admin.token).await.json();
    assert!(s.downloader_can_merge);
    let info: VideoInfo = h
        .call(
            Method::POST,
            "/api/tools/video/info",
            Some(&admin.token),
            Some(json!({"url": "https://videos.test/watch?v=1"})),
        )
        .await
        .json();
    let v = info.video.unwrap();
    // H.264 video-only (240p, not the 2160p above the cap) plus AAC audio.
    assert_eq!(
        (v.ext.as_str(), v.size, v.height),
        ("mp4", Some(2900), Some(240))
    );
    let r = h
        .call(
            Method::POST,
            "/api/tools/video/download",
            Some(&admin.token),
            Some(json!({"url": "https://videos.test/watch?v=1", "kind": "video"})),
        )
        .await;
    assert_eq!(r.status, StatusCode::OK);
    // Separate video and audio, fetched by two yt-dlp processes into FIFOs
    // and read by ffmpeg in order.
    assert_eq!(r.body, b"MERGED:VIDEO-ONLY-BYTES:AUDIO-ONLY-BYTES");
    // Audio is one file, sent as is.
    let r = h
        .call(
            Method::POST,
            "/api/tools/video/download",
            Some(&admin.token),
            Some(json!({"url": "https://videos.test/watch?v=1", "kind": "audio"})),
        )
        .await;
    assert_eq!(r.body, b"AUDIO-ONLY-BYTES");
    let mut files = Vec::new();
    all_files(&h.dir.path().join("data/downloads"), &mut files);
    assert!(files.is_empty(), "left behind: {files:?}");
}

// ---------------------------------------------------------------------------
// Two-factor sign-in: TOTP and passkeys (a software authenticator)
// ---------------------------------------------------------------------------

const ORIGIN: &str = "https://cloud.test";

fn base32_decode(s: &str) -> Vec<u8> {
    let (mut out, mut acc, mut bits) = (Vec::new(), 0u32, 0u32);
    for ch in s.bytes() {
        let v = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ234567"
            .iter()
            .position(|&x| x == ch)
            .unwrap() as u32;
        acc = (acc << 5) | v;
        bits += 5;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
        }
    }
    out
}

enum Signer {
    P256(ring::signature::EcdsaKeyPair),
    Ed25519(ring::signature::Ed25519KeyPair),
}

/// A passkey held in memory, standing in for a browser and authenticator.
struct SoftPasskey {
    id: Vec<u8>,
    signer: Signer,
    counter: u32,
}

fn cbor(v: &ciborium::Value) -> Vec<u8> {
    let mut out = Vec::new();
    ciborium::into_writer(v, &mut out).unwrap();
    out
}

fn client_data(kind: &str, challenge: &[u8], origin: &str) -> Vec<u8> {
    json!({"type": kind, "challenge": c::b64_encode(challenge), "origin": origin, "crossOrigin": false})
        .to_string()
        .into_bytes()
}

impl SoftPasskey {
    fn new(ed25519: bool) -> Self {
        use ring::signature::*;
        let rng = ring::rand::SystemRandom::new();
        let signer = if ed25519 {
            let pkcs8 = Ed25519KeyPair::generate_pkcs8(&rng).unwrap();
            Signer::Ed25519(Ed25519KeyPair::from_pkcs8(pkcs8.as_ref()).unwrap())
        } else {
            let alg = &ECDSA_P256_SHA256_ASN1_SIGNING;
            let pkcs8 = EcdsaKeyPair::generate_pkcs8(alg, &rng).unwrap();
            Signer::P256(EcdsaKeyPair::from_pkcs8(alg, pkcs8.as_ref(), &rng).unwrap())
        };
        SoftPasskey {
            id: c::random_bytes(16),
            signer,
            counter: 0,
        }
    }

    fn cose_key(&self) -> Vec<u8> {
        use ciborium::Value;
        use ring::signature::KeyPair;
        let i = |n: i64| Value::Integer(n.into());
        let map = match &self.signer {
            Signer::P256(k) => {
                let p = k.public_key().as_ref();
                vec![
                    (i(1), i(2)),
                    (i(3), i(-7)),
                    (i(-1), i(1)),
                    (i(-2), Value::Bytes(p[1..33].to_vec())),
                    (i(-3), Value::Bytes(p[33..].to_vec())),
                ]
            }
            Signer::Ed25519(k) => vec![
                (i(1), i(1)),
                (i(3), i(-8)),
                (i(-1), i(6)),
                (i(-2), Value::Bytes(k.public_key().as_ref().to_vec())),
            ],
        };
        cbor(&Value::Map(map))
    }

    fn auth_data(&self, flags: u8, attested: bool) -> Vec<u8> {
        let mut ad = thencloud_server::util::sha256(b"cloud.test");
        ad.push(flags);
        ad.extend(self.counter.to_be_bytes());
        if attested {
            ad.extend([0u8; 16]);
            ad.extend((self.id.len() as u16).to_be_bytes());
            ad.extend(&self.id);
            ad.extend(self.cose_key());
        }
        ad
    }

    fn create(&self, o: &PasskeyCreationOptions) -> (Vec<u8>, Vec<u8>) {
        use ciborium::Value;
        let att = Value::Map(vec![
            (Value::Text("fmt".into()), Value::Text("none".into())),
            (Value::Text("attStmt".into()), Value::Map(vec![])),
            (
                Value::Text("authData".into()),
                Value::Bytes(self.auth_data(0x45, true)),
            ),
        ]);
        (
            client_data("webauthn.create", &o.challenge, ORIGIN),
            cbor(&att),
        )
    }

    fn get(&mut self, challenge: &[u8], flags: u8, count: bool) -> PasskeyAssertion {
        if count {
            self.counter += 1;
        }
        let cd = client_data("webauthn.get", challenge, ORIGIN);
        let ad = self.auth_data(flags, false);
        let mut msg = ad.clone();
        msg.extend(thencloud_server::util::sha256(&cd));
        let signature = match &self.signer {
            Signer::P256(k) => k
                .sign(&ring::rand::SystemRandom::new(), &msg)
                .unwrap()
                .as_ref()
                .to_vec(),
            Signer::Ed25519(k) => k.sign(&msg).as_ref().to_vec(),
        };
        PasskeyAssertion {
            credential_id: B64(self.id.clone()),
            client_data_json: B64(cd),
            authenticator_data: B64(ad),
            signature: B64(signature),
            user_handle: None,
        }
    }
}

async fn password_login(h: &Harness, username: &str, password: &str) -> LoginResponse {
    let pre: PreloginResponse = h
        .call(
            Method::POST,
            "/api/auth/prelogin",
            None,
            Some(json!({ "username": username })),
        )
        .await
        .json();
    let ak = c::derive_account_keys(password, &pre.kdf_salt, pre.kdf_params).unwrap();
    let r = h
        .call(
            Method::POST,
            "/api/auth/login",
            None,
            Some(LoginRequest {
                username: username.into(),
                auth_key: B64(ak.auth_key.as_bytes().to_vec()),
                device_name: Some("laptop".into()),
            }),
        )
        .await;
    assert_eq!(r.status, StatusCode::OK, "{r:?}");
    r.json()
}

fn ticket(r: LoginResponse) -> SecondFactorChallenge {
    match r {
        LoginResponse::SecondFactor { second_factor } => second_factor,
        LoginResponse::Session(_) => panic!("expected a second factor request"),
    }
}

#[tokio::test]
async fn two_factor_sign_in_with_totp_and_passkeys() {
    let h = Harness::new().await;
    let password = "alice's long password";
    let alice = register(&h, "alice", password).await;
    let auth = || async { B64(current_auth(&h, password).await.as_bytes().to_vec()) };
    assert!(matches!(
        password_login(&h, "alice", password).await,
        LoginResponse::Session(_)
    ));

    // --- TOTP ----------------------------------------------------------------
    let r = h
        .call(
            Method::POST,
            "/api/auth/totp/setup",
            Some(&alice.token),
            Some(CurrentPassword {
                current_auth_key: B64(current_auth(&h, "wrong").await.as_bytes().to_vec()),
            }),
        )
        .await;
    assert_eq!(r.error(), "invalid_credentials");
    let setup: TotpSetup = h
        .call(
            Method::POST,
            "/api/auth/totp/setup",
            Some(&alice.token),
            Some(CurrentPassword {
                current_auth_key: auth().await,
            }),
        )
        .await
        .json();
    let secret = base32_decode(&setup.secret);
    assert_eq!(secret.len(), 20);
    let step = || thencloud_server::util::now() / thencloud_server::totp::STEP;
    let code = |s: i64| format!("{:06}", thencloud_server::totp::code(&secret, s));
    let enable = |code: String| EnableTotpRequest {
        setup_id: setup.setup_id.clone(),
        code,
    };
    let r = h
        .call(
            Method::POST,
            "/api/auth/totp",
            Some(&alice.token),
            Some(enable(code(step() + 5))),
        )
        .await;
    assert_eq!(r.error(), "invalid_second_factor");
    let r = h
        .call(
            Method::POST,
            "/api/auth/totp",
            Some(&alice.token),
            Some(enable(code(step()))),
        )
        .await;
    assert!(r.json::<Me>().totp_created_at.is_some());

    let t = ticket(password_login(&h, "alice", password).await);
    assert!(t.totp && t.passkey.is_none());
    let finish = |ticket: &str, totp_code: Option<String>, passkey: Option<PasskeyAssertion>| {
        let body = SecondFactorRequest {
            ticket: ticket.into(),
            totp_code,
            passkey,
        };
        let h = &h;
        async move {
            h.call(
                Method::POST,
                "/api/auth/login/second-factor",
                None,
                Some(body),
            )
            .await
        }
    };
    // The code used to turn it on doesn't work again.
    let r = finish(&t.ticket, Some(code(step())), None).await;
    assert_eq!(r.error(), "invalid_second_factor");
    let r = finish("made-up", Some(code(step() + 1)), None).await;
    assert_eq!(r.error(), "sign_in_expired");
    let next = code(step() + 1);
    let r = finish(&t.ticket, Some(next.clone()), None).await;
    assert_eq!(r.status, StatusCode::OK, "{r:?}");
    let s: SessionResponse = r.json();
    assert_eq!(h.get("/api/me", &s.token).await.status, StatusCode::OK);
    // A ticket works once, and so does a code.
    assert_eq!(
        finish(&t.ticket, Some(next.clone()), None).await.error(),
        "sign_in_expired"
    );
    let t2 = ticket(password_login(&h, "alice", password).await);
    assert_eq!(
        finish(&t2.ticket, Some(next), None).await.error(),
        "invalid_second_factor"
    );

    // --- passkeys ------------------------------------------------------------
    let mut key = SoftPasskey::new(false);
    let prf = c::random_bytes(32);
    let opts: PasskeyCreationOptions = h
        .call(
            Method::POST,
            "/api/passkeys/options",
            Some(&alice.token),
            None::<()>,
        )
        .await
        .json();
    assert_eq!(opts.user_handle.0, alice.me(&h).await.user_id.as_bytes());
    let (_, att) = key.create(&opts);
    let add =
        |registration_id: String, cd: Vec<u8>, att: Vec<u8>, auth: B64, wrap: Option<Vec<u8>>| {
            RegisterPasskeyRequest {
                registration_id,
                name: "Phone".into(),
                current_auth_key: auth,
                client_data_json: B64(cd),
                attestation_object: B64(att),
                enc_master_key: wrap.map(B64),
            }
        };
    let wrapped =
        c::wrap_master_key_passkey(&c::derive_passkey_kek(&prf).unwrap(), &alice.mk, &key.id);
    // Made on another site: refused.
    let bad_cd = client_data("webauthn.create", &opts.challenge, "https://evil.test");
    let r = h
        .call(
            Method::POST,
            "/api/passkeys",
            Some(&alice.token),
            Some(add(
                opts.registration_id.clone(),
                bad_cd,
                att.clone(),
                auth().await,
                None,
            )),
        )
        .await;
    assert_eq!(r.status, StatusCode::BAD_REQUEST, "{r:?}");
    // The registration was used up by that attempt.
    let opts: PasskeyCreationOptions = h
        .call(
            Method::POST,
            "/api/passkeys/options",
            Some(&alice.token),
            None::<()>,
        )
        .await
        .json();
    let (cd, att) = key.create(&opts);
    let r = h
        .call(
            Method::POST,
            "/api/passkeys",
            Some(&alice.token),
            Some(add(
                opts.registration_id.clone(),
                cd,
                att,
                auth().await,
                Some(wrapped.clone()),
            )),
        )
        .await;
    assert_eq!(r.status, StatusCode::CREATED, "{r:?}");
    let pk: Passkey = r.json();
    assert!(pk.unlock);

    // A second passkey without PRF: only a second factor.
    let mut plain = SoftPasskey::new(true);
    let opts: PasskeyCreationOptions = h
        .call(
            Method::POST,
            "/api/passkeys/options",
            Some(&alice.token),
            None::<()>,
        )
        .await
        .json();
    assert_eq!(opts.exclude_credentials, vec![B64(key.id.clone())]);
    let (cd, att) = plain.create(&opts);
    let r = h
        .call(
            Method::POST,
            "/api/passkeys",
            Some(&alice.token),
            Some(add(opts.registration_id, cd, att, auth().await, None)),
        )
        .await;
    assert_eq!(r.status, StatusCode::CREATED, "{r:?}");
    let listed: Vec<Passkey> = h.get("/api/passkeys", &alice.token).await.json();
    assert_eq!(
        listed.iter().map(|p| p.unlock).collect::<Vec<_>>(),
        [true, false]
    );

    // As a second factor, after the password.
    let t = ticket(password_login(&h, "alice", password).await);
    let req = t.passkey.unwrap();
    assert_eq!(req.allow_credentials.len(), 2);
    let r = finish(&t.ticket, None, Some(key.get(&req.challenge, 0x01, true))).await;
    assert_eq!(r.status, StatusCode::OK, "{r:?}");
    // A signature over another challenge doesn't count.
    let t = ticket(password_login(&h, "alice", password).await);
    let r = finish(&t.ticket, None, Some(plain.get(b"other", 0x01, false))).await;
    assert_eq!(r.error(), "invalid_second_factor");
    let r = finish(
        &t.ticket,
        None,
        Some(plain.get(&t.passkey.unwrap().challenge, 0x01, false)),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{r:?}");

    // On its own: the master key comes back wrapped under the PRF key.
    let login_with = |a: PasskeyAssertion, challenge: B64| {
        let h = &h;
        async move {
            h.call(
                Method::POST,
                "/api/auth/passkey/login",
                None,
                Some(PasskeyLoginRequest {
                    challenge,
                    assertion: a,
                    device_name: None,
                }),
            )
            .await
        }
    };
    let opts = || async {
        h.call(Method::POST, "/api/auth/passkey/options", None, None::<()>)
            .await
            .json::<PasskeyRequest>()
    };
    let o = opts().await;
    // User verification (PIN or biometric) is required without a password.
    let r = login_with(key.get(&o.challenge, 0x01, true), o.challenge.clone()).await;
    assert_eq!(r.error(), "invalid_credentials");
    let a = key.get(&o.challenge, 0x05, true);
    let r = login_with(a.clone(), o.challenge.clone()).await;
    assert_eq!(r.status, StatusCode::OK, "{r:?}");
    let s: PasskeyLoginResponse = r.json();
    let mk = c::unwrap_master_key_passkey(
        &c::derive_passkey_kek(&prf).unwrap(),
        &s.enc_master_key,
        &s.credential_id,
    )
    .unwrap();
    assert!(mk == alice.mk);
    assert_eq!(h.get("/api/me", &s.token).await.status, StatusCode::OK);
    // Replayed: the counter didn't move on.
    assert_eq!(
        login_with(a, o.challenge.clone()).await.error(),
        "invalid_credentials"
    );
    // A challenge the server didn't make.
    let mut forged = o.challenge.0.clone();
    forged[30] ^= 1;
    assert_eq!(
        login_with(key.get(&forged, 0x05, true), B64(forged))
            .await
            .error(),
        "sign_in_expired"
    );
    // One that has been used, by a passkey that always counts zero.
    let o = opts().await;
    let a = plain.get(&o.challenge, 0x05, false);
    let r = login_with(a, o.challenge).await;
    assert_eq!(
        r.status,
        StatusCode::BAD_REQUEST,
        "no PRF, so no unlock: {r:?}"
    );
    let o = opts().await;
    let a = key.get(&o.challenge, 0x05, true);
    assert_eq!(
        login_with(a.clone(), o.challenge.clone()).await.status,
        StatusCode::OK
    );
    let again = key.get(&o.challenge, 0x05, true);
    assert_eq!(
        login_with(again, o.challenge).await.error(),
        "sign_in_expired"
    );

    // Nothing that opens the account is stored.
    let mut files = Vec::new();
    all_files(&h.dir.path().join("data"), &mut files);
    for p in &files {
        let bytes = std::fs::read(p).unwrap();
        for n in [&prf[..], alice.mk.as_bytes()] {
            assert!(!contains(&bytes, n), "secret found in {}", p.display());
        }
    }

    // Removing them needs the password; then a password is enough again.
    for p in &listed {
        let r = h
            .call(
                Method::DELETE,
                &format!("/api/passkeys/{}", p.id),
                Some(&alice.token),
                Some(CurrentPassword {
                    current_auth_key: auth().await,
                }),
            )
            .await;
        assert_eq!(r.status, StatusCode::NO_CONTENT);
    }
    let r = h
        .call(
            Method::DELETE,
            "/api/auth/totp",
            Some(&alice.token),
            Some(CurrentPassword {
                current_auth_key: auth().await,
            }),
        )
        .await;
    assert!(r.json::<Me>().totp_created_at.is_none());
    assert!(matches!(
        password_login(&h, "alice", password).await,
        LoginResponse::Session(_)
    ));
}

#[tokio::test]
async fn post_quantum_keys_seal_shares_and_drops() {
    let h = Harness::new().await;
    let alice = register(&h, "alice", "pw").await;

    // An account from before post-quantum keys gets one on its next sign-in.
    let salt = c::random_bytes(c::SALT_LEN);
    let ak = c::derive_account_keys("pw", &salt, FAST_KDF).unwrap();
    let (mk, kp, root_id, root_key) = (
        Key::generate(),
        KeyPair::generate(),
        c::new_id(),
        Key::generate(),
    );
    let r = h
        .call(
            Method::POST,
            "/api/auth/register",
            None,
            Some(RegisterRequest {
                username: "old".into(),
                auth_key: B64(ak.auth_key.as_bytes().to_vec()),
                kdf_salt: B64(salt),
                kdf_params: FAST_KDF,
                enc_master_key: B64(c::wrap_master_key(&ak.kek, &mk)),
                public_key: B64(kp.public.to_vec()),
                enc_private_key: B64(c::wrap_private_key(&mk, &kp.secret)),
                pq_public_key: None,
                enc_pq_private_key: None,
                root: NewRootFolder {
                    id: root_id.clone(),
                    enc_key: B64(c::wrap_node_key(&mk, &root_key, &root_id)),
                    enc_metadata: B64(
                        c::encrypt_metadata(&root_key, &root_id, &meta("root", 0)).unwrap()
                    ),
                },
                device_name: None,
                invite: None,
            }),
        )
        .await;
    let old: SessionResponse = r.json();
    let pk: UserPublicKey = h
        .get("/api/users/old/public-key", &alice.token)
        .await
        .json();
    assert!(pk.pq_public_key.is_none());
    // Shares to it are classic until then.
    assert_eq!(sealing_key(&pk).len(), 32);

    let pq = c::PqKeyPair::generate();
    let set = |public: Vec<u8>, wrapped: Vec<u8>| SetPqKeyRequest {
        pq_public_key: B64(public),
        enc_pq_private_key: B64(wrapped),
    };
    let r = h
        .call(
            Method::PUT,
            "/api/me/pq-key",
            Some(&old.token),
            Some(set(
                pq.public[..100].to_vec(),
                c::wrap_pq_private_key(&mk, &pq),
            )),
        )
        .await;
    assert_eq!(r.status, StatusCode::BAD_REQUEST);
    let r = h
        .call(
            Method::PUT,
            "/api/me/pq-key",
            Some(&old.token),
            Some(set(pq.public.clone(), c::wrap_pq_private_key(&mk, &pq))),
        )
        .await;
    let me: Me = r.json();
    let kp =
        kp.with_pq(c::unwrap_pq_private_key(&mk, &me.keys.enc_pq_private_key.unwrap()).unwrap());
    // It is never replaced, so a server can't be asked to swap it.
    let r = h
        .call(
            Method::PUT,
            "/api/me/pq-key",
            Some(&old.token),
            Some(set(
                c::PqKeyPair::generate().public.clone(),
                c::wrap_pq_private_key(&mk, &pq),
            )),
        )
        .await;
    assert_eq!(r.status, StatusCode::CONFLICT);

    // Now shares to it are hybrid and open with both halves.
    let pk: UserPublicKey = h
        .get("/api/users/old/public-key", &alice.token)
        .await
        .json();
    assert_eq!(pk.pq_public_key.as_deref(), Some(&pq.public[..]));
    let (folder, folder_key) = alice.mkdir(&h, &alice.root, "Quantum-safe").await;
    let wrapped = c::seal_share_key(&sealing_key(&pk), &folder_key, &folder).unwrap();
    assert_eq!(wrapped.len(), thencloud_server::util::HYBRID_SEALED_KEY_LEN);
    let r = h
        .call(
            Method::POST,
            "/api/shares",
            Some(&alice.token),
            Some(CreateShareRequest {
                node_id: folder.clone(),
                recipient: "old".into(),
                wrapped_key: B64(wrapped),
                permission: Permission::Read,
                expires_at: None,
            }),
        )
        .await;
    assert_eq!(r.status, StatusCode::CREATED, "{r:?}");
    let shares: Vec<IncomingShare> = h.get("/api/shares/incoming", &old.token).await.json();
    assert_eq!(
        shares[0].owner_pq_public_key.as_deref(),
        Some(&alice.kp.pq.as_ref().unwrap().public[..])
    );
    assert!(c::open_share_key(&kp, &shares[0].wrapped_key, &folder).unwrap() == folder_key);
    // A wrong-sized sealed key is refused.
    let r = h
        .call(
            Method::POST,
            "/api/shares",
            Some(&alice.token),
            Some(CreateShareRequest {
                node_id: folder.clone(),
                recipient: "old".into(),
                wrapped_key: B64(vec![2; 500]),
                permission: Permission::Read,
                expires_at: None,
            }),
        )
        .await;
    assert_eq!(r.status, StatusCode::BAD_REQUEST);

    // A drop link names the hash of the owner's ML-KEM key after `#`; the
    // key itself comes from the server and must match it.
    let (inbox, _) = alice.mkdir(&h, &alice.root, "Inbox").await;
    let link: Link = h
        .call(
            Method::POST,
            "/api/links",
            Some(&alice.token),
            Some(CreateLinkRequest {
                node_id: inbox.clone(),
                password_auth: None,
                enc_link_key: None,
                enc_link_secret: None,
                expires_at: None,
                upload_only: true,
                max_opens: None,
            }),
        )
        .await
        .json();
    let info: PublicLinkInfo = h
        .call(
            Method::GET,
            &format!("/api/public/{}", link.token),
            None,
            None::<()>,
        )
        .await
        .json();
    let alice_pq = info.owner_pq_public_key.unwrap();
    let fragment = c::identity(
        &alice.kp.public,
        Some(&alice.kp.pq.as_ref().unwrap().public),
    );
    assert_eq!(c::identity(&fragment[..32], Some(&alice_pq)), fragment);
    let owner_key = [&fragment[..32], &alice_pq[..]].concat();
    let (id, k, st) = drop_file(&h, &link.token, &owner_key, &inbox, "q.txt", b"quantum").await;
    assert_eq!(st, StatusCode::NO_CONTENT);
    let drops: Vec<DroppedFile> = h.get("/api/drops", &alice.token).await.json();
    assert!(c::open_drop_key(&alice.kp, &drops[0].sealed_key, &id, &inbox).unwrap() == k);

    // No ML-KEM seed is stored in the clear.
    let mut files = Vec::new();
    all_files(&h.dir.path().join("data"), &mut files);
    for p in &files {
        let bytes = std::fs::read(p).unwrap();
        for seed in [pq.seed(), alice.kp.pq.as_ref().unwrap().seed()] {
            assert!(!contains(&bytes, seed), "ML-KEM seed in {}", p.display());
        }
    }
}

#[tokio::test]
async fn links_with_limited_opens() {
    let h = Harness::new().await;
    let alice = register(&h, "alice", "pw").await;
    let (folder, folder_key) = alice.mkdir(&h, &alice.root, "Once only").await;
    let file = alice
        .upload(&h, &folder, None, "note.txt", b"read me once")
        .await
        .unwrap();
    let (with_password, secret) = link_with_password(&folder, &folder_key, "sesame");
    let create = |max_opens, password: Option<&str>, upload_only| match password {
        Some(_) => CreateLinkRequest {
            max_opens,
            ..with_password.clone()
        },
        None => CreateLinkRequest {
            node_id: folder.clone(),
            password_auth: None,
            enc_link_key: None,
            enc_link_secret: None,
            expires_at: None,
            upload_only,
            max_opens,
        },
    };
    for bad in [create(Some(0), None, false), create(Some(3), None, true)] {
        let r = h
            .call(Method::POST, "/api/links", Some(&alice.token), Some(bad))
            .await;
        assert_eq!(r.status, StatusCode::BAD_REQUEST);
    }

    // Two opens. Each visit gets a token that covers the rest of it.
    let link: Link = h
        .call(
            Method::POST,
            "/api/links",
            Some(&alice.token),
            Some(create(Some(2), None, false)),
        )
        .await
        .json();
    assert_eq!((link.max_opens, link.opens), (Some(2), 0));
    let base = format!("/api/public/{}", link.token);
    let open = || async {
        h.raw(Method::GET, &base, None, &[], Body::empty(), None)
            .await
    };
    // Without a visit's token nothing below the link is served.
    let r = h
        .raw(
            Method::GET,
            &format!("{base}/nodes/{folder}/children"),
            None,
            &[],
            Body::empty(),
            None,
        )
        .await;
    assert_eq!(r.status, StatusCode::NOT_FOUND);

    let first: PublicLinkInfo = open().await.json();
    let visit = first.link_token.expect("a visit token");
    let vt = [("x-link-token", visit.as_str())];
    // Asking again within the visit doesn't count another open.
    let again: PublicLinkInfo = h
        .raw(Method::GET, &base, None, &vt, Body::empty(), None)
        .await
        .json();
    assert!(again.link_token.is_none());
    let kids: Vec<Node> = h
        .raw(
            Method::GET,
            &format!("{base}/nodes/{folder}/children"),
            None,
            &vt,
            Body::empty(),
            None,
        )
        .await
        .json();
    assert_eq!(kids.len(), 1);

    let second = open().await;
    assert_eq!(second.status, StatusCode::OK);
    // Used up: gone for anyone new...
    assert_eq!(open().await.status, StatusCode::NOT_FOUND);
    // ...but the first visit can still finish its download.
    let fk = c::unwrap_node_key(&folder_key, &file.enc_key, &file.id).unwrap();
    let (_, got) = download_via(
        &h,
        &file,
        &fk,
        |i| format!("{base}/nodes/{}/chunks/{i}", file.id),
        None,
        &vt,
    )
    .await;
    assert_eq!(got, b"read me once");
    let listed: Vec<Link> = h.get("/api/links", &alice.token).await.json();
    assert_eq!(listed[0].opens, 2);

    // With a password, the open is counted after it, and the visit's token
    // stands for the password too.
    let link: Link = h
        .call(
            Method::POST,
            "/api/links",
            Some(&alice.token),
            Some(create(Some(1), Some("sesame"), false)),
        )
        .await
        .json();
    let base = format!("/api/public/{}", link.token);
    let r = h
        .raw(Method::GET, &base, None, &[], Body::empty(), None)
        .await;
    assert_eq!(r.status, StatusCode::UNAUTHORIZED);
    let unlocked: UnlockLinkResponse = h
        .call(
            Method::POST,
            &format!("{base}/unlock"),
            None,
            Some(unlock_body(&secret, "sesame")),
        )
        .await
        .json();
    let pt = [("x-link-token", unlocked.link_token.as_str())];
    // A password token alone doesn't reach the files of a counted link.
    let r = h
        .raw(
            Method::GET,
            &format!("{base}/nodes/{folder}/children"),
            None,
            &pt,
            Body::empty(),
            None,
        )
        .await;
    assert_eq!(r.status, StatusCode::NOT_FOUND);
    let info: PublicLinkInfo = h
        .raw(Method::GET, &base, None, &pt, Body::empty(), None)
        .await
        .json();
    let visit = info.link_token.unwrap();
    let r = h
        .raw(
            Method::GET,
            &format!("{base}/nodes/{folder}/children"),
            None,
            &[("x-link-token", visit.as_str())],
            Body::empty(),
            None,
        )
        .await;
    assert_eq!(r.status, StatusCode::OK);
    let r = h
        .raw(Method::GET, &base, None, &pt, Body::empty(), None)
        .await;
    assert_eq!(r.status, StatusCode::NOT_FOUND);

    // Counting opens stores numbers, nothing else.
    let mut files = Vec::new();
    all_files(&h.dir.path().join("data"), &mut files);
    for p in &files {
        let bytes = std::fs::read(p).unwrap();
        for n in [
            &b"Once only"[..],
            b"note.txt",
            b"read me once",
            b"sesame",
            folder_key.as_bytes(),
        ] {
            assert!(!contains(&bytes, n), "plaintext found in {}", p.display());
        }
    }
}

#[tokio::test]
async fn user_shares_can_expire() {
    let h = Harness::new().await;
    let alice = register(&h, "alice", "pw").await;
    let bob = register(&h, "bob", "pw").await;
    let (folder, folder_key) = alice.mkdir(&h, &alice.root, "For a week").await;
    let pk: UserPublicKey = h
        .get("/api/users/bob/public-key", &alice.token)
        .await
        .json();
    let share = |expires_at| CreateShareRequest {
        node_id: folder.clone(),
        recipient: "bob".into(),
        wrapped_key: B64(c::seal_share_key(&sealing_key(&pk), &folder_key, &folder).unwrap()),
        permission: Permission::Read,
        expires_at,
    };
    let t = thencloud_server::util::now();
    let r = h
        .call(
            Method::POST,
            "/api/shares",
            Some(&alice.token),
            Some(share(Some(t - 1))),
        )
        .await;
    assert_eq!(r.status, StatusCode::BAD_REQUEST);
    let r = h
        .call(
            Method::POST,
            "/api/shares",
            Some(&alice.token),
            Some(share(Some(t + 7 * 86400))),
        )
        .await;
    assert_eq!(r.status, StatusCode::CREATED);
    let out: OutgoingShare = r.json();
    assert_eq!(out.expires_at, Some(t + 7 * 86400));
    let incoming: Vec<IncomingShare> = h.get("/api/shares/incoming", &bob.token).await.json();
    assert_eq!(incoming[0].expires_at, Some(t + 7 * 86400));
    assert_eq!(bob.children(&h, &folder).await.status, StatusCode::OK);

    // The week passes.
    sqlx::query("UPDATE shares SET expires_at = ?")
        .bind(t - 1)
        .execute(&h.state.db)
        .await
        .unwrap();
    assert_eq!(
        bob.children(&h, &folder).await.status,
        StatusCode::NOT_FOUND
    );
    assert!(
        h.get("/api/shares/incoming", &bob.token)
            .await
            .json::<Vec<IncomingShare>>()
            .is_empty()
    );
    assert!(
        h.get("/api/shares/outgoing", &alice.token)
            .await
            .json::<Vec<OutgoingShare>>()
            .is_empty()
    );
    thencloud_server::janitor::run_once(&h.state).await.unwrap();
    let left: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM shares")
        .fetch_one(&h.state.db)
        .await
        .unwrap();
    assert_eq!(left, 0);

    // Sharing again with no expiry works as before.
    let r = h
        .call(
            Method::POST,
            "/api/shares",
            Some(&alice.token),
            Some(share(None)),
        )
        .await;
    assert_eq!(r.status, StatusCode::CREATED);
    assert_eq!(bob.children(&h, &folder).await.status, StatusCode::OK);
}

async fn auth_of(h: &Harness, username: &str, password: &str) -> Key {
    let pre: PreloginResponse = h
        .call(
            Method::POST,
            "/api/auth/prelogin",
            None,
            Some(json!({ "username": username })),
        )
        .await
        .json();
    c::derive_account_keys(password, &pre.kdf_salt, pre.kdf_params)
        .unwrap()
        .auth_key
}

async fn delete_account(h: &Harness, token: &str, key: Key) -> Resp {
    h.call(
        Method::POST,
        "/api/me/delete",
        Some(token),
        Some(DeleteAccountRequest {
            current_auth_key: B64(key.as_bytes().to_vec()),
        }),
    )
    .await
}

#[tokio::test]
async fn people_can_delete_their_own_account() {
    let h = Harness::new().await;
    let alice = register(&h, "alice", "alice-pw").await; // the first, so an admin
    let bob = register(&h, "bob", "bob-pw").await;
    let (folder, folder_key) = bob.mkdir(&h, &bob.root, "Bob's things").await;
    bob.upload(&h, &folder, None, "diary.txt", b"dear diary")
        .await
        .unwrap();
    let pk: UserPublicKey = h
        .get("/api/users/alice/public-key", &bob.token)
        .await
        .json();
    let r = h
        .call(
            Method::POST,
            "/api/shares",
            Some(&bob.token),
            Some(CreateShareRequest {
                node_id: folder.clone(),
                recipient: "alice".into(),
                wrapped_key: B64(
                    c::seal_share_key(&sealing_key(&pk), &folder_key, &folder).unwrap()
                ),
                permission: Permission::Read,
                expires_at: None,
            }),
        )
        .await;
    assert_eq!(r.status, StatusCode::CREATED);

    // The wrong password is refused.
    let r = delete_account(&h, &bob.token, auth_of(&h, "alice", "alice-pw").await).await;
    assert_eq!(r.status, StatusCode::UNAUTHORIZED);
    // The only admin can't leave the others without one.
    let r = delete_account(&h, &alice.token, auth_of(&h, "alice", "alice-pw").await).await;
    assert_eq!(r.status, StatusCode::BAD_REQUEST);

    let used_before: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM nodes")
        .fetch_one(&h.state.db)
        .await
        .unwrap();
    let r = delete_account(&h, &bob.token, auth_of(&h, "bob", "bob-pw").await).await;
    assert_eq!(r.status, StatusCode::NO_CONTENT);
    // Signed out, gone, and so is everything of theirs.
    assert_eq!(
        h.get("/api/me", &bob.token).await.status,
        StatusCode::UNAUTHORIZED
    );
    assert!(login(&h, "bob", "bob-pw").await.is_err());
    assert!(
        h.get("/api/shares/incoming", &alice.token)
            .await
            .json::<Vec<IncomingShare>>()
            .is_empty()
    );
    let left: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM nodes")
        .fetch_one(&h.state.db)
        .await
        .unwrap();
    assert_eq!(left, used_before - 3, "bob's root, folder and file");
    let mut files = Vec::new();
    all_files(&h.dir.path().join("data/blobs"), &mut files);
    assert!(files.is_empty(), "bob's blobs removed: {files:?}");

    // With nobody else left, the admin can go too.
    let r = delete_account(&h, &alice.token, auth_of(&h, "alice", "alice-pw").await).await;
    assert_eq!(r.status, StatusCode::NO_CONTENT);
}

#[tokio::test]
async fn health_check_and_metrics() {
    let h = Harness::new().await;
    let r = h
        .raw(Method::GET, "/api/health", None, &[], Body::empty(), None)
        .await;
    assert_eq!(r.status, StatusCode::OK);
    // Metrics are off without a token configured.
    let r = h
        .raw(Method::GET, "/api/metrics", None, &[], Body::empty(), None)
        .await;
    assert_eq!(r.status, StatusCode::NOT_FOUND);

    let h = Harness::with_config(|c| c.metrics_token = Some("scrape-me-please".into())).await;
    let alice = register(&h, "alice", "pw").await;
    alice
        .upload(&h, &alice.root, None, "counted.txt", b"hello")
        .await
        .unwrap();
    for bad in [None, Some("Bearer nope")] {
        let headers: Vec<(&str, &str)> = bad.map(|b| ("authorization", b)).into_iter().collect();
        let r = h
            .raw(
                Method::GET,
                "/api/metrics",
                None,
                &headers,
                Body::empty(),
                None,
            )
            .await;
        assert_eq!(r.status, StatusCode::UNAUTHORIZED);
    }
    let r = h
        .raw(
            Method::GET,
            "/api/metrics",
            Some("scrape-me-please"),
            &[],
            Body::empty(),
            None,
        )
        .await;
    assert_eq!(r.status, StatusCode::OK);
    let text = String::from_utf8(r.body).unwrap();
    assert!(text.contains("# TYPE thencloud_users gauge\nthencloud_users 1\n"));
    assert!(text.contains("\nthencloud_files 1\n"));
    assert!(!text.contains("counted"));
}

#[tokio::test]
async fn backup_restore_and_check() {
    use thencloud_server::maintenance;
    let h = Harness::new().await;
    let alice = register(&h, "alice", "pw").await;
    let (folder, folder_key) = alice.mkdir(&h, &alice.root, "Kept").await;
    let data = secret_payload(c::CHUNK_SIZE + 99);
    let file = alice
        .upload(&h, &folder, None, "keep.bin", &data)
        .await
        .unwrap();
    let r = maintenance::check(&h.state.db, &h.state.blobs)
        .await
        .unwrap();
    assert!(r.is_ok(), "{r:?}");
    assert_eq!((r.versions, r.chunks), (1, 2));

    let dest = h.dir.path().join("backup");
    let dest = maintenance::BackupDest::Dir(dest);
    let b = maintenance::backup(&h.state.db, &h.state.blobs, &dest)
        .await
        .unwrap();
    assert_eq!(b.chunks, 2);
    assert!(b.missing.is_empty());
    // A second backup into the same place is refused.
    assert!(
        maintenance::backup(&h.state.db, &h.state.blobs, &dest)
            .await
            .is_err()
    );

    // Changes after the backup don't reach it.
    alice.delete(&h, &file.id).await;
    let r = h
        .call(Method::DELETE, "/api/trash", Some(&alice.token), None::<()>)
        .await;
    assert_eq!(r.status, StatusCode::NO_CONTENT);

    // Restore: a server started on the backup has the file back.
    let mut cfg = Config::for_dir(h.dir.path());
    let maintenance::BackupDest::Dir(dest_dir) = &dest else {
        unreachable!()
    };
    cfg.data_dir = dest_dir.clone();
    let state = AppState::new(cfg).await.unwrap();
    let restored = Harness {
        app: router(state.clone()),
        state,
        dir: tempfile::tempdir().unwrap(),
    };
    let alice2 = login(&restored, "alice", "pw").await.unwrap();
    let kids: Vec<Node> = alice2.children(&restored, &folder).await.json();
    let f = find_by_name(&kids, &folder_key, "keep.bin");
    let fk = c::unwrap_node_key(&folder_key, &f.enc_key, &f.id).unwrap();
    assert_eq!(alice2.download(&restored, f, &fk).await.1, data);
    let r = maintenance::check(&restored.state.db, &restored.state.blobs)
        .await
        .unwrap();
    assert!(r.is_ok() && r.orphans.is_empty(), "{r:?}");

    // A lost or truncated blob, and one nothing refers to, are reported.
    let version = f.version.as_ref().unwrap().id.clone();
    let dest = dest_dir.clone();
    let vdir = dest.join("blobs").join(&version[..2]).join(&version);
    std::fs::write(vdir.join("1"), b"short").unwrap();
    std::fs::remove_file(vdir.join("0")).unwrap();
    let stray = dest.join("blobs/ab/abcdef00-0000-4000-8000-000000000000");
    std::fs::create_dir_all(&stray).unwrap();
    let r = maintenance::check(&restored.state.db, &restored.state.blobs)
        .await
        .unwrap();
    assert!(!r.is_ok());
    assert_eq!(r.missing, vec![(version.clone(), 0)]);
    assert_eq!(
        r.orphans,
        vec!["abcdef00-0000-4000-8000-000000000000".to_string()]
    );

    // Nothing in the backup is plaintext.
    let mut files = Vec::new();
    all_files(&dest, &mut files);
    for p in &files {
        let bytes = std::fs::read(p).unwrap();
        for n in [MARKER, b"keep.bin", b"Kept", folder_key.as_bytes()] {
            assert!(!contains(&bytes, n), "plaintext in {}", p.display());
        }
    }
}

#[tokio::test]
async fn behind_a_proxy_rate_limits_use_forwarded_for() {
    async fn attempt(h: &Harness, username: &str, key: &Key, from: &str) -> StatusCode {
        let body = serde_json::to_vec(&LoginRequest {
            username: username.into(),
            auth_key: B64(key.as_bytes().to_vec()),
            device_name: None,
        })
        .unwrap();
        h.raw(
            Method::POST,
            "/api/auth/login",
            None,
            &[("x-forwarded-for", from)],
            Body::from(body),
            Some("application/json"),
        )
        .await
        .status
    }
    for trust in [false, true] {
        let h = Harness::with_config(|c| c.trust_proxy = trust).await;
        register(&h, "erin", "pw").await;
        let good = auth_of(&h, "erin", "pw").await;
        let bad = Key::generate();
        for i in 0..10 {
            let st = attempt(&h, &format!("nobody{i}"), &bad, "10.0.0.1, 203.0.113.5").await;
            assert_eq!(st, StatusCode::UNAUTHORIZED);
        }
        // The same client is now blocked, whatever it claims further left.
        assert_eq!(
            attempt(&h, "erin", &good, "198.51.100.9, 203.0.113.5").await,
            StatusCode::TOO_MANY_REQUESTS
        );
        // Another client behind the same proxy isn't, but only when the
        // proxy is trusted; otherwise the header means nothing.
        let other = attempt(&h, "erin", &good, "203.0.113.77").await;
        let expected = if trust {
            StatusCode::OK
        } else {
            StatusCode::TOO_MANY_REQUESTS
        };
        assert_eq!(other, expected, "trust_proxy = {trust}");
    }
}

#[tokio::test]
async fn onion_services_limit_by_account_not_by_address() {
    // Behind Tor every visitor arrives from the same address: failures
    // from strangers mustn't lock everyone else out.
    let h = Harness::with_config(|c| c.limit_by_address = false).await;
    let frank = register(&h, "frank", "pw").await;
    let bad = Key::generate();
    for i in 0..15 {
        let r = h
            .call(
                Method::POST,
                "/api/auth/login",
                None,
                Some(LoginRequest {
                    username: format!("stranger{i}"),
                    auth_key: B64(bad.as_bytes().to_vec()),
                    device_name: None,
                }),
            )
            .await;
        assert_eq!(r.status, StatusCode::UNAUTHORIZED);
    }
    assert!(login(&h, "frank", "pw").await.is_ok());

    // Guessing one account's password is still limited.
    for _ in 0..10 {
        assert!(login(&h, "frank", "wrong").await.is_err());
    }
    assert_eq!(
        login(&h, "frank", "pw").await.err().unwrap().status,
        StatusCode::TOO_MANY_REQUESTS
    );

    // So is guessing a link's password: per link, as there are no addresses.
    let (folder, folder_key) = frank.mkdir(&h, &frank.root, "Shared").await;
    let (req, secret) = link_with_password(&folder, &folder_key, "right");
    let link: Link = h
        .call(Method::POST, "/api/links", Some(&frank.token), Some(req))
        .await
        .json();
    let (wrong, right) = (unlock_body(&secret, "wrong"), unlock_body(&secret, "right"));
    let unlock = |body: &serde_json::Value| {
        let uri = format!("/api/public/{}/unlock", link.token);
        let (h, body) = (&h, body.clone());
        async move { h.call(Method::POST, &uri, None, Some(body)).await.status }
    };
    for _ in 0..10 {
        assert_eq!(unlock(&wrong).await, StatusCode::UNAUTHORIZED);
    }
    assert_eq!(unlock(&right).await, StatusCode::TOO_MANY_REQUESTS);
}

#[tokio::test]
async fn the_server_records_times_to_the_hour() {
    let h = Harness::new().await;
    let a = register(&h, "grace", "pw").await;
    let (folder, _) = a.mkdir(&h, &a.root, "Timed").await;
    let f = a.upload(&h, &folder, None, "t.txt", b"one").await.unwrap();
    let f = a.upload(&h, "", Some(&f), "t.txt", b"two").await.unwrap();
    for v in a.versions(&h, &f.id).await {
        assert_eq!(v.created_at % 3600, 0);
    }
    a.delete(&h, &folder).await;
    let rows: Vec<(i64, i64, Option<i64>)> =
        sqlx::query_as("SELECT created_at, updated_at, trashed_at FROM nodes")
            .fetch_all(&h.state.db)
            .await
            .unwrap();
    assert_eq!(rows.len(), 3); // root, folder, file
    for (created, updated, trashed) in rows {
        assert_eq!(created % 3600, 0);
        assert_eq!(updated % 3600, 0);
        assert!(trashed.is_none_or(|t| t % 3600 == 0));
    }
}

#[tokio::test]
async fn comments_are_read_by_those_with_access_and_no_one_else() {
    let h = Harness::new().await;
    let alice = register(&h, "alice", "pw").await;
    let bob = register(&h, "bob", "pw").await;
    let carol = register(&h, "carol", "pw").await;
    let (alice_id, bob_id) = (alice.me(&h).await.user_id, bob.me(&h).await.user_id);
    let (folder, folder_key) = alice.mkdir(&h, &alice.root, "Reviewed").await;
    let file = alice
        .upload(&h, &folder, None, "draft.txt", b"text")
        .await
        .unwrap();
    let file_key = alice.key_of(&h, &file.id).await;
    let pk: UserPublicKey = h
        .get("/api/users/bob/public-key", &alice.token)
        .await
        .json();
    let r = h
        .call(
            Method::POST,
            "/api/shares",
            Some(&alice.token),
            Some(CreateShareRequest {
                node_id: folder.clone(),
                recipient: "bob".into(),
                wrapped_key: B64(
                    c::seal_share_key(&sealing_key(&pk), &folder_key, &folder).unwrap()
                ),
                permission: Permission::Read,
                expires_at: None,
            }),
        )
        .await;
    assert_eq!(r.status, StatusCode::CREATED);

    let uri = format!("/api/nodes/{}/comments", file.id);
    let post = |who: &Client, author_id: &str, text: &str| {
        let id = c::new_id();
        let body = c::encrypt_comment(&file_key, &file.id, &id, author_id, text.as_bytes());
        (
            who.token.clone(),
            CreateCommentRequest {
                id,
                enc_body: B64(body),
            },
        )
    };
    let (t, req) = post(&alice, &alice_id, "COMMENT-SECRET looks good");
    let r = h.call(Method::POST, &uri, Some(&t), Some(&req)).await;
    assert_eq!(r.status, StatusCode::CREATED);
    // The same id again is refused.
    assert_eq!(
        h.call(Method::POST, &uri, Some(&t), Some(&req))
            .await
            .status,
        StatusCode::CONFLICT
    );
    // A reader can comment too.
    let (t, req) = post(&bob, &bob_id, "COMMENT-SECRET one typo");
    let bobs: Comment = h
        .call(Method::POST, &uri, Some(&t), Some(&req))
        .await
        .json();
    assert_eq!(bobs.author, "bob");

    // Bob reads both, in order; each opens only in its own name.
    let list: Vec<Comment> = h.get(&uri, &bob.token).await.json();
    assert_eq!(list.len(), 2);
    let text = |cm: &Comment| {
        String::from_utf8(
            c::decrypt_comment(&file_key, &file.id, &cm.id, &cm.author_id, &cm.enc_body).unwrap(),
        )
        .unwrap()
    };
    assert_eq!(text(&list[0]), "COMMENT-SECRET looks good");
    assert_eq!(text(&list[1]), "COMMENT-SECRET one typo");
    assert!(
        c::decrypt_comment(&file_key, &file.id, &list[0].id, &bob_id, &list[0].enc_body).is_err(),
        "a comment can't be put in someone else's name"
    );
    assert!(
        c::decrypt_comment(
            &file_key,
            &folder,
            &list[0].id,
            &alice_id,
            &list[0].enc_body
        )
        .is_err(),
        "or moved to another node"
    );

    // Carol has no access: she can't see them, add one, or delete one.
    assert_eq!(
        h.get(&uri, &carol.token).await.status,
        StatusCode::NOT_FOUND
    );
    let (t, req) = post(&carol, "carol", "hello");
    assert_eq!(
        h.call(Method::POST, &uri, Some(&t), Some(&req))
            .await
            .status,
        StatusCode::NOT_FOUND
    );
    let del = |id: &str| format!("/api/comments/{id}");
    assert_eq!(
        h.call(
            Method::DELETE,
            &del(&list[0].id),
            Some(&carol.token),
            None::<()>
        )
        .await
        .status,
        StatusCode::NOT_FOUND
    );
    // Bob can't delete Alice's, only his own; Alice owns the file and can delete any.
    assert_eq!(
        h.call(
            Method::DELETE,
            &del(&list[0].id),
            Some(&bob.token),
            None::<()>
        )
        .await
        .status,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        h.call(
            Method::DELETE,
            &del(&bobs.id),
            Some(&alice.token),
            None::<()>
        )
        .await
        .status,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        h.get(&uri, &alice.token).await.json::<Vec<Comment>>().len(),
        1
    );

    // The text never reaches the disk; deleting the file takes its comments.
    let mut files = Vec::new();
    all_files(&h.dir.path().join("data"), &mut files);
    for p in &files {
        assert!(
            !contains(&std::fs::read(p).unwrap(), b"COMMENT-SECRET"),
            "{}",
            p.display()
        );
    }
    assert_eq!(alice.delete(&h, &file.id).await, StatusCode::NO_CONTENT);
    assert_eq!(
        h.get(&uri, &alice.token).await.status,
        StatusCode::NOT_FOUND
    );
}

#[tokio::test]
async fn thumbnails_follow_the_current_version() {
    let h = Harness::new().await;
    let alice = register(&h, "alice", "pw").await;
    let bob = register(&h, "bob", "pw").await;
    let (folder, _) = alice.mkdir(&h, &alice.root, "Photos").await;
    let file = alice
        .upload(&h, &folder, None, "cat.jpg", b"not really a jpeg")
        .await
        .unwrap();
    let key = alice.key_of(&h, &file.id).await;
    let v1 = file.version.clone().unwrap();
    assert!(!v1.has_thumbnail);
    let get = format!("/api/nodes/{}/thumbnail", file.id);
    assert_eq!(
        h.get(&get, &alice.token).await.status,
        StatusCode::NOT_FOUND
    );

    let put = |vid: &str| format!("/api/nodes/{}/versions/{vid}/thumbnail", file.id);
    let thumb = c::encrypt_thumbnail(&key, &file.id, &v1.id, b"THUMB-SECRET small jpeg");
    let send = |uri: String, token: String, body: Vec<u8>| {
        let h = &h;
        async move {
            h.raw(Method::PUT, &uri, Some(&token), &[], Body::from(body), None)
                .await
                .status
        }
    };
    // Only someone who can write the file, and only a thumbnail-sized one.
    assert_eq!(
        send(put(&v1.id), bob.token.clone(), thumb.clone()).await,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        send(put(&v1.id), alice.token.clone(), vec![0; 70 * 1024]).await,
        StatusCode::PAYLOAD_TOO_LARGE
    );
    assert_eq!(
        send(put("no-such-version"), alice.token.clone(), thumb.clone()).await,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        send(put(&v1.id), alice.token.clone(), thumb.clone()).await,
        StatusCode::NO_CONTENT
    );
    let node: Node = h
        .get(&format!("/api/nodes/{}", file.id), &alice.token)
        .await
        .json();
    assert!(node.version.as_ref().unwrap().has_thumbnail);
    let r = h.get(&get, &alice.token).await;
    assert_eq!(r.status, StatusCode::OK);
    assert_eq!(
        c::decrypt_thumbnail(&key, &file.id, &v1.id, &r.body).unwrap(),
        b"THUMB-SECRET small jpeg"
    );

    // A new version has none until one is made for it; the old one can't
    // stand in for it, since it's bound to its version.
    let v2 = alice
        .upload(&h, "", Some(&node), "cat.jpg", b"another")
        .await
        .unwrap();
    let v2_id = v2.version.as_ref().unwrap().id.clone();
    assert!(!v2.version.unwrap().has_thumbnail);
    assert_eq!(
        h.get(&get, &alice.token).await.status,
        StatusCode::NOT_FOUND
    );
    assert!(c::decrypt_thumbnail(&key, &file.id, &v2_id, &thumb).is_err());

    // Through a public link, as far as the link reaches.
    assert_eq!(
        send(
            put(&v2_id),
            alice.token.clone(),
            c::encrypt_thumbnail(&key, &file.id, &v2_id, b"THUMB-SECRET v2")
        )
        .await,
        StatusCode::NO_CONTENT
    );
    let link: Link = h
        .call(
            Method::POST,
            "/api/links",
            Some(&alice.token),
            Some(CreateLinkRequest {
                node_id: folder.clone(),
                password_auth: None,
                enc_link_key: None,
                enc_link_secret: None,
                expires_at: None,
                upload_only: false,
                max_opens: None,
            }),
        )
        .await
        .json();
    let r = h
        .raw(
            Method::GET,
            &format!("/api/public/{}/nodes/{}/thumbnail", link.token, file.id),
            None,
            &[],
            Body::empty(),
            None,
        )
        .await;
    assert_eq!(r.status, StatusCode::OK);
    assert_eq!(
        c::decrypt_thumbnail(&key, &file.id, &v2_id, &r.body).unwrap(),
        b"THUMB-SECRET v2"
    );

    let mut files = Vec::new();
    all_files(&h.dir.path().join("data"), &mut files);
    for p in &files {
        assert!(
            !contains(&std::fs::read(p).unwrap(), b"THUMB-SECRET"),
            "{}",
            p.display()
        );
    }
}

#[tokio::test]
async fn activity_shows_who_did_what_to_those_who_can_see_the_folder() {
    let h = Harness::new().await;
    let alice = register(&h, "alice", "pw").await;
    let bob = register(&h, "bob", "pw").await;
    let carol = register(&h, "carol", "pw").await;
    let (team, team_key) = alice.mkdir(&h, &alice.root, "Team").await;
    let (inner, _) = alice.mkdir(&h, &team, "Drafts").await;
    let (elsewhere, elsewhere_key) = alice.mkdir(&h, &alice.root, "Private").await;
    let pk: UserPublicKey = h
        .get("/api/users/bob/public-key", &alice.token)
        .await
        .json();
    let r = h
        .call(
            Method::POST,
            "/api/shares",
            Some(&alice.token),
            Some(CreateShareRequest {
                node_id: team.clone(),
                recipient: "bob".into(),
                wrapped_key: B64(c::seal_share_key(&sealing_key(&pk), &team_key, &team).unwrap()),
                permission: Permission::Write,
                expires_at: None,
            }),
        )
        .await;
    assert_eq!(r.status, StatusCode::CREATED);

    // Bob adds a file deep in the share and saves it twice; Alice renames
    // it, then moves it out to a folder Bob can't see, then trashes the
    // inner folder.
    let file = bob
        .upload(&h, &inner, None, "plan.txt", b"one")
        .await
        .unwrap();
    let file = bob
        .upload(&h, "", Some(&file), "plan.txt", b"two")
        .await
        .unwrap();
    bob.upload(&h, "", Some(&file), "plan.txt", b"three")
        .await
        .unwrap();
    let file_key = alice.key_of(&h, &file.id).await;
    let meta = c::encrypt_metadata(&file_key, &file.id, &meta("plan-v2.txt", 5)).unwrap();
    let node: Node = h
        .get(&format!("/api/nodes/{}", file.id), &alice.token)
        .await
        .json();
    let r = h
        .call(
            Method::PATCH,
            &format!("/api/nodes/{}", file.id),
            Some(&alice.token),
            Some(UpdateNodeRequest {
                enc_metadata: Some(B64(meta)),
                parent_id: None,
                enc_key: None,
                if_revision: Some(node.revision),
                name_tag: None,
            }),
        )
        .await;
    assert_eq!(r.status, StatusCode::OK, "{r:?}");
    let r = h
        .call(
            Method::PATCH,
            &format!("/api/nodes/{}", file.id),
            Some(&alice.token),
            Some(UpdateNodeRequest {
                enc_metadata: None,
                parent_id: Some(elsewhere.clone()),
                enc_key: Some(B64(c::wrap_node_key(&elsewhere_key, &file_key, &file.id))),
                if_revision: None,
                name_tag: None,
            }),
        )
        .await;
    assert_eq!(r.status, StatusCode::OK, "{r:?}");
    assert_eq!(alice.delete(&h, &inner).await, StatusCode::NO_CONTENT);

    let list = |who: &Client, folder: &str| {
        let (h, uri, token) = (
            &h,
            format!("/api/nodes/{folder}/activity"),
            who.token.clone(),
        );
        async move { h.get(&uri, &token).await }
    };
    let events: Vec<ActivityEvent> = list(&bob, &team).await.json();
    let seen: Vec<(String, String, String)> = events
        .iter()
        .map(|e| (e.actor.clone(), e.kind.clone(), e.node_id.clone()))
        .collect();
    let ev = |a: &str, k: &str, n: &str| (a.to_string(), k.to_string(), n.to_string());
    // Newest first. The two saves in one hour are one "changed"; everything
    // stays in Team's history though the file has left it.
    assert_eq!(
        seen,
        vec![
            ev("alice", "trashed", &inner),
            ev("alice", "moved", &file.id),
            ev("alice", "renamed", &file.id),
            ev("bob", "changed", &file.id),
            ev("bob", "added", &file.id),
            ev("alice", "added", &inner),
        ]
    );
    assert!(events[0].folder && !events[1].folder);
    assert_eq!(events[0].at % 3600, 0, "to the hour");
    // Paging.
    let older: Vec<ActivityEvent> = h
        .get(
            &format!("/api/nodes/{team}/activity?before={}", events[2].id),
            &bob.token,
        )
        .await
        .json();
    assert_eq!(older.len(), 3);
    // The move shows where it went too, but only to those who can see there.
    let private: Vec<ActivityEvent> = list(&alice, &elsewhere).await.json();
    assert_eq!(private.len(), 1);
    assert_eq!(private[0].kind, "moved");
    assert_eq!(list(&bob, &elsewhere).await.status, StatusCode::NOT_FOUND);
    assert_eq!(list(&carol, &team).await.status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn live_changes_reach_those_watching_the_folder() {
    let h = Harness::new().await;
    let alice = register(&h, "alice", "pw").await;
    let bob = register(&h, "bob", "pw").await;
    let (team, _) = alice.mkdir(&h, &alice.root, "Team").await;
    let (inner, _) = alice.mkdir(&h, &team, "Inner").await;
    let (other, _) = alice.mkdir(&h, &alice.root, "Other").await;
    let uri = format!("/api/nodes/{team}/changes");
    assert_eq!(h.get(&uri, &bob.token).await.status, StatusCode::NOT_FOUND);

    let req = Request::builder()
        .uri(&uri)
        .header("authorization", format!("Bearer {}", alice.token))
        .body(Body::empty())
        .unwrap();
    let res = h.app.clone().oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    assert!(
        res.headers()[axum::http::header::CONTENT_TYPE]
            .to_str()
            .unwrap()
            .starts_with("text/event-stream")
    );
    let mut body = res.into_body();

    // A change elsewhere, then one deep inside the folder being watched.
    alice
        .upload(&h, &other, None, "elsewhere.txt", b"x")
        .await
        .unwrap();
    let file = alice
        .upload(&h, &inner, None, "LIVE-SECRET.txt", b"y")
        .await
        .unwrap();
    let mut got = String::new();
    while !got.contains("\n\n") {
        let frame = tokio::time::timeout(std::time::Duration::from_secs(5), body.frame())
            .await
            .expect("a change arrives")
            .unwrap()
            .unwrap();
        if let Ok(data) = frame.into_data() {
            got.push_str(std::str::from_utf8(&data).unwrap());
        }
    }
    assert!(got.starts_with("event: change\n"), "{got}");
    assert!(
        got.contains(&file.id),
        "only the watched folder's change: {got}"
    );
    assert!(!got.contains("LIVE-SECRET"), "ids only: {got}");

    // A new folder counts too.
    let (made, _) = alice.mkdir(&h, &team, "New").await;
    let mut got = String::new();
    while !got.contains("\n\n") {
        let frame = tokio::time::timeout(std::time::Duration::from_secs(5), body.frame())
            .await
            .expect("a new folder arrives")
            .unwrap()
            .unwrap();
        if let Ok(data) = frame.into_data() {
            got.push_str(std::str::from_utf8(&data).unwrap());
        }
    }
    assert!(got.contains(&made), "{got}");
}

#[tokio::test]
async fn the_search_index_may_be_bigger_than_other_app_data() {
    let h = Harness::new().await;
    let ada = register(&h, "ada", "pw").await;
    let big = vec![7u8; 3 * 1024 * 1024];
    let put = |name: &str| {
        let (h, uri, token, big) = (
            &h,
            format!("/api/me/data/{name}"),
            ada.token.clone(),
            big.clone(),
        );
        async move {
            h.call(
                Method::PUT,
                &uri,
                Some(&token),
                Some(PutPrivateData {
                    data: B64(big),
                    if_revision: 0,
                }),
            )
            .await
            .status
        }
    };
    assert_eq!(put("music").await, StatusCode::BAD_REQUEST);
    assert_eq!(put("search").await, StatusCode::OK);
}

#[tokio::test]
async fn the_change_feed_lists_what_changed_after_a_cursor() {
    let h = Harness::new().await;
    let alice = register(&h, "alice", "pw").await;
    let bob = register(&h, "bob", "pw").await;
    let feed = |who: &Client, since: Option<i64>| {
        let uri = match since {
            Some(s) => format!("/api/changes?since={s}"),
            None => "/api/changes".to_string(),
        };
        let (h, token) = (&h, who.token.clone());
        async move {
            let r = h.get(&uri, &token).await;
            assert_eq!(r.status, StatusCode::OK, "{r:?}");
            r.json::<ChangeFeed>()
        }
    };
    fn ids(f: &ChangeFeed) -> Vec<&str> {
        f.changes.iter().map(|c| c.node_id.as_str()).collect()
    }

    // Without a cursor: just where things stand now.
    let start = feed(&alice, None).await;
    assert!(start.changes.is_empty() && !start.resync);
    let bob_start = feed(&bob, None).await.cursor;

    let (team, team_key) = alice.mkdir(&h, &alice.root, "Team").await;
    let (private, private_key) = alice.mkdir(&h, &alice.root, "Private").await;
    let pk: UserPublicKey = h
        .get("/api/users/bob/public-key", &alice.token)
        .await
        .json();
    let r = h
        .call(
            Method::POST,
            "/api/shares",
            Some(&alice.token),
            Some(CreateShareRequest {
                node_id: team.clone(),
                recipient: "bob".into(),
                wrapped_key: B64(c::seal_share_key(&sealing_key(&pk), &team_key, &team).unwrap()),
                permission: Permission::Write,
                expires_at: None,
            }),
        )
        .await;
    assert_eq!(r.status, StatusCode::CREATED);
    let shared = alice.upload(&h, &team, None, "a.txt", b"a").await.unwrap();
    let secret = alice
        .upload(&h, &private, None, "b.txt", b"b")
        .await
        .unwrap();
    let edited = bob
        .upload(&h, "", Some(&shared), "a.txt", b"a2")
        .await
        .unwrap();

    // Alice sees everything in her tree, in order, whoever did it.
    let all = feed(&alice, Some(start.cursor)).await;
    assert_eq!(
        ids(&all),
        [
            team.as_str(),
            private.as_str(),
            shared.id.as_str(),
            secret.id.as_str(),
            edited.id.as_str()
        ]
    );
    assert!(!all.more && !all.resync);
    assert!(all.changes.windows(2).all(|w| w[0].seq < w[1].seq));
    // Nothing new since then.
    let again = feed(&alice, Some(all.cursor)).await;
    assert!(again.changes.is_empty() && again.cursor == all.cursor);

    // Bob sees only what's under the folder shared with him.
    let bobs = feed(&bob, Some(bob_start)).await;
    assert_eq!(
        ids(&bobs),
        [team.as_str(), shared.id.as_str(), shared.id.as_str()]
    );
    assert_eq!(bobs.cursor, all.cursor, "past what he can't see too");

    // A move out of the shared folder shows to him (it left), and nothing
    // after it in the private folder does.
    let file_key = alice.key_of(&h, &shared.id).await;
    let r = h
        .call(
            Method::PATCH,
            &format!("/api/nodes/{}", shared.id),
            Some(&alice.token),
            Some(UpdateNodeRequest {
                enc_metadata: None,
                parent_id: Some(private.clone()),
                enc_key: Some(B64(c::wrap_node_key(&private_key, &file_key, &shared.id))),
                if_revision: None,
                name_tag: None,
            }),
        )
        .await;
    assert_eq!(r.status, StatusCode::OK, "{r:?}");
    alice
        .upload(&h, "", Some(&secret), "b.txt", b"b2")
        .await
        .unwrap();
    let bobs = feed(&bob, Some(bobs.cursor)).await;
    assert_eq!(ids(&bobs), [shared.id.as_str()]);
    let later = feed(&alice, Some(all.cursor)).await;
    assert_eq!(ids(&later), [shared.id.as_str(), secret.id.as_str()]);

    // A cursor from the future, or from before what's kept: start over.
    assert!(feed(&alice, Some(later.cursor + 5)).await.resync);
    sqlx::query("UPDATE changes SET at = 0 WHERE seq <= ?")
        .bind(all.cursor)
        .execute(&h.state.db)
        .await
        .unwrap();
    thencloud_server::janitor::run_once(&h.state).await.unwrap();
    let old = feed(&alice, Some(start.cursor)).await;
    assert!(old.resync && old.changes.is_empty());
    assert_eq!(old.cursor, later.cursor);
    let kept = feed(&alice, Some(all.cursor)).await;
    assert!(!kept.resync);
    assert_eq!(ids(&kept), [shared.id.as_str(), secret.id.as_str()]);
    // Pruning took the scope rows with it.
    let orphans: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM change_scope WHERE seq NOT IN (SELECT seq FROM changes)",
    )
    .fetch_one(&h.state.db)
    .await
    .unwrap();
    assert_eq!(orphans, 0);
}

#[tokio::test]
async fn big_folders_are_listed_a_page_at_a_time() {
    let h = Harness::new().await;
    let alice = register(&h, "alice", "pw").await;
    let (big, _) = alice.mkdir(&h, &alice.root, "Big").await;
    for i in 0..6 {
        alice.mkdir(&h, &big, &format!("dir {i}")).await;
    }
    for i in 0..11 {
        alice
            .upload(&h, &big, None, &format!("f{i}.txt"), b"x")
            .await
            .unwrap();
    }
    let all: Vec<Node> = h
        .get(&format!("/api/nodes/{big}/children"), &alice.token)
        .await
        .json();
    assert_eq!(all.len(), 17);

    // Pages of 5 add up to the whole listing, in the same order.
    let mut paged = Vec::new();
    let mut after: Option<String> = None;
    let mut pages = 0;
    loop {
        let uri = match &after {
            Some(a) => format!("/api/nodes/{big}/children?limit=5&after={a}"),
            None => format!("/api/nodes/{big}/children?limit=5"),
        };
        let r = h.get(&uri, &alice.token).await;
        assert_eq!(r.status, StatusCode::OK, "{r:?}");
        let page: NodePage = r.json();
        assert!(page.nodes.len() <= 5);
        paged.extend(page.nodes);
        pages += 1;
        match page.next {
            Some(n) => after = Some(n),
            None => break,
        }
    }
    assert_eq!(pages, 4);
    let ids = |v: &[Node]| v.iter().map(|n| n.id.clone()).collect::<Vec<_>>();
    assert_eq!(ids(&paged), ids(&all));

    // Nonsense cursors are refused, not guessed at.
    for bad in ["x", "file.notanumber.id"] {
        let r = h
            .get(
                &format!("/api/nodes/{big}/children?limit=5&after={bad}"),
                &alice.token,
            )
            .await;
        assert_eq!(r.status, StatusCode::BAD_REQUEST, "{bad}");
    }

    // A page is read from the index in order, not sorted afterwards.
    let plan: Vec<(i64, i64, i64, String)> = sqlx::query_as(
        "EXPLAIN QUERY PLAN SELECT id FROM nodes n WHERE n.parent_id = ?1 AND n.trashed_at IS NULL \
         AND n.dropped = 0 AND (?2 = 0 OR n.kind < ?3) ORDER BY n.kind DESC, n.created_at, n.id LIMIT 5",
    )
    .bind(&big)
    .bind(false)
    .bind("")
    .fetch_all(&h.state.db)
    .await
    .unwrap();
    let plan: Vec<&str> = plan.iter().map(|p| p.3.as_str()).collect();
    assert!(
        plan.iter().any(|p| p.contains("nodes_children")),
        "{plan:?}"
    );
    assert!(!plan.iter().any(|p| p.contains("TEMP B-TREE")), "{plan:?}");
}

#[tokio::test]
async fn admins_can_limit_how_much_someone_moves_a_day() {
    let h = Harness::new().await;
    let admin = register(&h, "root", "admin password").await;
    let bob = register(&h, "bob", "bob's password").await;
    let bob_id = bob.me(&h).await.user_id;
    let file = bob
        .upload(&h, &bob.root, None, "a.txt", b"some bytes")
        .await
        .unwrap();
    let chunk = format!("/api/nodes/{}/chunks/0", file.id);
    let limit = |body: serde_json::Value| {
        let (h, uri, token) = (
            &h,
            format!("/api/admin/users/{bob_id}"),
            admin.token.clone(),
        );
        async move {
            h.call(Method::PATCH, &uri, Some(&token), Some(body))
                .await
                .json::<AdminUser>()
        }
    };

    // No limit by default.
    for _ in 0..3 {
        assert_eq!(h.get(&chunk, &bob.token).await.status, StatusCode::OK);
    }
    let u = limit(json!({})).await;
    assert!(u.daily_download_limit.is_none() && u.downloaded_today > 0);

    // Over today's download limit: refused, with a reason, until it's lifted.
    let u = limit(json!({"daily_download_limit": u.downloaded_today})).await;
    assert_eq!(u.daily_download_limit, Some(u.downloaded_today));
    let r = h.get(&chunk, &bob.token).await;
    assert_eq!(r.status, StatusCode::TOO_MANY_REQUESTS);
    assert!(String::from_utf8_lossy(&r.body).contains("transfer_limit"));
    let mine: TransferInfo = h.get("/api/me/transfer", &bob.token).await.json();
    assert_eq!(mine.daily_download_limit, u.daily_download_limit);
    assert!(mine.resets_at > now_secs() && mine.resets_at % 86_400 == 0);
    limit(json!({"daily_download_limit": 0})).await;
    assert_eq!(h.get(&chunk, &bob.token).await.status, StatusCode::OK);

    // Uploads the same way.
    let u = limit(json!({"daily_upload_limit": 1})).await;
    assert!(u.uploaded_today > 0);
    let r = bob.upload(&h, &bob.root, None, "b.txt", b"more").await;
    assert_eq!(r.unwrap_err().status, StatusCode::TOO_MANY_REQUESTS);

    // Only admins set them.
    let r = h
        .call(
            Method::PATCH,
            &format!("/api/admin/users/{bob_id}"),
            Some(&bob.token),
            Some(json!({"daily_upload_limit": 0})),
        )
        .await;
    assert_eq!(r.status, StatusCode::FORBIDDEN);
}

fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64
// --- a fake S3 server, for the blob store's S3 side ------------------------
//
// Just enough of the REST API for the AWS SDK: path-style object PUT/GET/
// HEAD/DELETE, batch delete and ListObjectsV2 with delimiter, max-keys and
// continuation tokens. Signatures are ignored; the point is the client side.

mod fake_s3 {
    use std::collections::BTreeMap;
    use std::sync::Arc;

    use axum::extract::Request;
    use axum::http::{Method, StatusCode, header};
    use axum::response::{IntoResponse, Response};
    use http_body_util::BodyExt;
    use tokio::sync::Mutex;

    /// Objects by "bucket/key".
    pub type Store = Arc<Mutex<BTreeMap<String, Vec<u8>>>>;

    /// Start the server; returns its store and endpoint URL.
    pub async fn spawn() -> (Store, String) {
        let store: Store = Arc::new(Mutex::new(BTreeMap::new()));
        let app = axum::Router::new().fallback({
            let store = store.clone();
            move |req: Request| {
                let store = store.clone();
                async move { handle(store, req).await }
            }
        });
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        (store, url)
    }

    fn percent_decode(s: &str) -> String {
        let b = s.as_bytes();
        let mut out = Vec::with_capacity(b.len());
        let mut i = 0;
        while i < b.len() {
            if b[i] == b'%' && i + 2 < b.len() {
                out.push(u8::from_str_radix(&s[i + 1..i + 3], 16).unwrap());
                i += 3;
            } else if b[i] == b'+' {
                out.push(b' ');
                i += 1;
            } else {
                out.push(b[i]);
                i += 1;
            }
        }
        String::from_utf8(out).unwrap()
    }

    fn query_pairs(uri: &axum::http::Uri) -> Vec<(String, String)> {
        uri.query()
            .unwrap_or("")
            .split('&')
            .filter(|s| !s.is_empty())
            .map(|kv| match kv.split_once('=') {
                Some((k, v)) => (percent_decode(k), percent_decode(v)),
                None => (percent_decode(kv), String::new()),
            })
            .collect()
    }

    fn xml(body: String) -> Response {
        (
            StatusCode::OK,
            [(header::CONTENT_TYPE, "application/xml")],
            body,
        )
            .into_response()
    }

    fn no_such_key() -> Response {
        (
            StatusCode::NOT_FOUND,
            [(header::CONTENT_TYPE, "application/xml")],
            r#"<?xml version="1.0" encoding="UTF-8"?><Error><Code>NoSuchKey</Code><Message>not found</Message></Error>"#,
        )
            .into_response()
    }

    enum Entry {
        Key(String, usize),
        Prefix(String),
    }

    async fn list(store: &Store, bucket: &str, q: &[(String, String)]) -> Response {
        let get = |k: &str| q.iter().find(|(kk, _)| kk == k).map(|(_, v)| v.clone());
        let prefix = get("prefix").unwrap_or_default();
        let delimiter = get("delimiter").unwrap_or_default();
        let max_keys: usize = get("max-keys").and_then(|s| s.parse().ok()).unwrap_or(1000);
        let token: usize = get("continuation-token")
            .and_then(|s| s.parse().ok())
            .unwrap_or(0);
        let full_prefix = format!("{bucket}/{prefix}");
        let mut entries: Vec<Entry> = Vec::new();
        {
            let map = store.lock().await;
            let mut commons: Vec<String> = Vec::new();
            for (k, v) in map.iter() {
                if !k.starts_with(&full_prefix) {
                    continue;
                }
                let rel = &k[bucket.len() + 1..];
                let tail = &rel[prefix.len()..];
                if delimiter == "/" && tail.contains('/') {
                    let p = format!("{prefix}{}/", &tail[..tail.find('/').unwrap()]);
                    if !commons.contains(&p) {
                        commons.push(p.clone());
                        entries.push(Entry::Prefix(p));
                    }
                } else {
                    entries.push(Entry::Key(rel.to_string(), v.len()));
                }
            }
        }
        entries.sort_by_key(|e| match e {
            Entry::Key(k, _) => k.clone(),
            Entry::Prefix(p) => p.clone(),
        });
        let total = entries.len();
        let page: Vec<Entry> = entries.into_iter().skip(token).take(max_keys).collect();
        let truncated = token + page.len() < total;
        let mut out = format!(
            r#"<?xml version="1.0" encoding="UTF-8"?><ListBucketResult xmlns="http://s3.amazonaws.com/doc/2006-03-01/"><Name>{bucket}</Name><Prefix>{prefix}</Prefix><MaxKeys>{max_keys}</MaxKeys><KeyCount>{}</KeyCount><IsTruncated>{truncated}</IsTruncated>"#,
            page.len()
        );
        if truncated {
            out += &format!(
                "<NextContinuationToken>{}</NextContinuationToken>",
                token + page.len()
            );
        }
        for e in &page {
            match e {
                Entry::Key(k, size) => {
                    out += &format!(
                        r#"<Contents><Key>{k}</Key><Size>{size}</Size><LastModified>2026-01-01T00:00:00.000Z</LastModified><ETag>&quot;fake&quot;</ETag><StorageClass>STANDARD</StorageClass></Contents>"#
                    )
                }
                Entry::Prefix(p) => {
                    out += &format!("<CommonPrefixes><Prefix>{p}</Prefix></CommonPrefixes>")
                }
            }
        }
        out += "</ListBucketResult>";
        xml(out)
    }

    pub async fn handle(store: Store, req: Request) -> Response {
        let (parts, body) = req.into_parts();
        let body = body.collect().await.unwrap().to_bytes().to_vec();
        let q = query_pairs(&parts.uri);
        let has = |k: &str| q.iter().any(|(kk, _)| kk == k);
        let path = percent_decode(parts.uri.path());
        let trimmed = path.trim_start_matches('/');
        let (bucket, key) = match trimmed.split_once('/') {
            Some((b, k)) => (b.to_string(), k.to_string()),
            None => (trimmed.to_string(), String::new()),
        };
        let map_key = format!("{bucket}/{key}");
        match parts.method {
            Method::PUT => {
                store.lock().await.insert(map_key, body);
                (StatusCode::OK, [(header::ETAG, "\"fake\"")]).into_response()
            }
            Method::GET if has("list-type") => list(&store, &bucket, &q).await,
            Method::GET => match store.lock().await.get(&map_key) {
                Some(v) => (
                    StatusCode::OK,
                    [
                        (header::CONTENT_TYPE, "application/octet-stream"),
                        (header::ETAG, "\"fake\""),
                    ],
                    v.clone(),
                )
                    .into_response(),
                None => no_such_key(),
            },
            Method::HEAD => match store.lock().await.get(&map_key) {
                Some(v) => (
                    StatusCode::OK,
                    [(header::CONTENT_LENGTH, v.len().to_string())],
                )
                    .into_response(),
                None => StatusCode::NOT_FOUND.into_response(),
            },
            Method::POST if has("delete") => {
                let text = String::from_utf8_lossy(&body).into_owned();
                let mut out = String::from(
                    r#"<?xml version="1.0" encoding="UTF-8"?><DeleteResult xmlns="http://s3.amazonaws.com/doc/2006-03-01/">"#,
                );
                let mut rest = &text[..];
                let mut map = store.lock().await;
                while let Some(start) = rest.find("<Key>") {
                    let body_start = start + 5;
                    let end = body_start + rest[body_start..].find("</Key>").unwrap();
                    let k = &rest[body_start..end];
                    map.remove(&format!("{bucket}/{k}"));
                    out += &format!("<Deleted><Key>{k}</Key></Deleted>");
                    rest = &rest[end..];
                }
                out += "</DeleteResult>";
                xml(out)
            }
            Method::DELETE => {
                store.lock().await.remove(&map_key);
                StatusCode::NO_CONTENT.into_response()
            }
            _ => StatusCode::NOT_IMPLEMENTED.into_response(),
        }
    }
}

#[tokio::test]
async fn s3_blob_store_backup_and_restore() {
    use thencloud_server::maintenance::{self, BackupDest};

    let (store, endpoint) = fake_s3::spawn().await;
    let h = Harness::with_config(|c| {
        c.s3_endpoint = Some(endpoint.clone());
        c.s3_bucket = Some("main".into());
        c.s3_access_key = Some("access".into());
        c.s3_secret_key = Some("secret".into());
        c.s3_prefix = "tc/".into();
    })
    .await;
    let alice = register(&h, "alice", "pw").await;
    let (folder, folder_key) = alice.mkdir(&h, &alice.root, "Kept").await;
    let data = secret_payload(c::CHUNK_SIZE + 99);
    let file = alice
        .upload(&h, &folder, None, "keep.bin", &data)
        .await
        .unwrap();

    // The chunks went to the bucket, not to the data directory.
    assert!(!h.dir.path().join("data/blobs").exists());
    async fn count_under(store: &fake_s3::Store, p: &str) -> usize {
        store
            .lock()
            .await
            .keys()
            .filter(|k| k.starts_with(p))
            .count()
    }
    assert_eq!(count_under(&store, "main/tc/").await, 2);

    // Downloading reads them back.
    let kids: Vec<Node> = alice.children(&h, &folder).await.json();
    let f = find_by_name(&kids, &folder_key, "keep.bin");
    let fk = c::unwrap_node_key(&folder_key, &f.enc_key, &f.id).unwrap();
    assert_eq!(alice.download(&h, f, &fk).await.1, data);

    // Zero-knowledge: the bucket holds nothing but ciphertext.
    for (k, v) in store.lock().await.iter() {
        for n in [MARKER, b"keep.bin", b"Kept", folder_key.as_bytes()] {
            assert!(
                !contains(v, n),
                "plaintext {:?} in {k}",
                String::from_utf8_lossy(n)
            );
        }
    }

    // check() works against the bucket, and sees an orphan put behind its back.
    let r = maintenance::check(&h.state.db, &h.state.blobs)
        .await
        .unwrap();
    assert!(r.is_ok() && r.orphans.is_empty(), "{r:?}");
    store.lock().await.insert(
        "main/tc/ab/abcdef00-0000-4000-8000-000000000000/0".into(),
        vec![1, 2, 3],
    );
    let r = maintenance::check(&h.state.db, &h.state.blobs)
        .await
        .unwrap();
    assert_eq!(
        r.orphans,
        vec!["abcdef00-0000-4000-8000-000000000000".to_string()]
    );
    store
        .lock()
        .await
        .remove("main/tc/ab/abcdef00-0000-4000-8000-000000000000/0");

    // Back up bucket -> another prefix in the same bucket.
    let cfg = &h.state.config;
    let dest = BackupDest::parse("s3://backups/one", cfg).unwrap();
    let b = maintenance::backup(&h.state.db, &h.state.blobs, &dest)
        .await
        .unwrap();
    assert_eq!(b.chunks, 2);
    assert!(b.missing.is_empty());
    assert!(store.lock().await.contains_key("backups/one/thencloud.db"));
    // A second backup into the same place is refused.
    assert!(
        maintenance::backup(&h.state.db, &h.state.blobs, &dest)
            .await
            .is_err()
    );

    // Restore: the backed-up db in a fresh data directory, blobs from the
    // backup's own prefix.
    let rdir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(rdir.path().join("data")).unwrap();
    let db_bytes = store
        .lock()
        .await
        .get("backups/one/thencloud.db")
        .unwrap()
        .clone();
    std::fs::write(rdir.path().join("data/thencloud.db"), db_bytes).unwrap();
    let mut cfg2 = Config::for_dir(rdir.path());
    cfg2.s3_endpoint = Some(endpoint.clone());
    cfg2.s3_bucket = Some("backups".into());
    cfg2.s3_access_key = Some("access".into());
    cfg2.s3_secret_key = Some("secret".into());
    cfg2.s3_prefix = "one/blobs/".into();
    let state = AppState::new(cfg2).await.unwrap();
    let restored = Harness {
        app: router(state.clone()),
        state,
        dir: rdir,
    };
    let alice2 = login(&restored, "alice", "pw").await.unwrap();
    let kids: Vec<Node> = alice2.children(&restored, &folder).await.json();
    let f = find_by_name(&kids, &folder_key, "keep.bin");
    let fk = c::unwrap_node_key(&folder_key, &f.enc_key, &f.id).unwrap();
    assert_eq!(alice2.download(&restored, f, &fk).await.1, data);
    let r = maintenance::check(&restored.state.db, &restored.state.blobs)
        .await
        .unwrap();
    assert!(r.is_ok() && r.orphans.is_empty(), "{r:?}");

    // Back up bucket -> a local directory, and check what landed there.
    let ldir = h.dir.path().join("backup");
    let b = maintenance::backup(&h.state.db, &h.state.blobs, &BackupDest::Dir(ldir.clone()))
        .await
        .unwrap();
    assert_eq!(b.chunks, 2);
    assert!(ldir.join("thencloud.db").exists());
    let version = f.version.as_ref().unwrap().id.clone();
    assert!(
        ldir.join("blobs")
            .join(&version[..2])
            .join(&version)
            .join("0")
            .exists()
    );

    // Emptying the trash deletes the blobs from the bucket.
    assert_eq!(alice.delete(&h, &file.id).await, StatusCode::NO_CONTENT);
    let r = h
        .call(Method::DELETE, "/api/trash", Some(&alice.token), None::<()>)
        .await;
    assert_eq!(r.status, StatusCode::NO_CONTENT);
    assert_eq!(count_under(&store, "main/tc/").await, 0);
}
