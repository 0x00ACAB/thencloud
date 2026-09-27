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

fn meta(name: &str, size: u64) -> Metadata {
    Metadata {
        name: name.into(),
        mime: Some("application/octet-stream".into()),
        size,
        mtime: 1_700_000_000_000,
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
) -> Result<Client, Resp> {
    let salt = c::random_bytes(c::SALT_LEN);
    let ak = c::derive_account_keys(password, &salt, FAST_KDF).unwrap();
    let mk = Key::generate();
    let kp = KeyPair::generate();
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
        return Err(r);
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

async fn login(h: &Harness, username: &str, password: &str) -> Result<Client, Resp> {
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
        return Err(r);
    }
    let s: SessionResponse = r.json();
    let mk = c::unwrap_master_key(&ak.kek, &s.me.keys.enc_master_key).unwrap();
    let kp = c::unwrap_private_key(&mk, &s.me.keys.enc_private_key).unwrap();
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
    ) -> Result<Node, Resp> {
        let (node_id, node_key, enc_key) = match existing {
            Some(n) => (n.id.clone(), self.key_of(h, &n.id).await, None),
            None => {
                let pk = self.key_of(h, parent).await;
                let id = c::new_id();
                let k = Key::generate();
                let ek = B64(c::wrap_node_key(&pk, &k, &id));
                (id, k, Some(ek))
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
        };
        let r = h
            .call(Method::POST, "/api/uploads", Some(&self.token), Some(&req))
            .await;
        if r.status != StatusCode::CREATED {
            return Err(r);
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
                return Err(r);
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
            return Err(r);
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
    assert_eq!(out.len() as u64, m.size);
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
        wrapped_key: B64(c::seal_share_key(&pk.public_key, &other_key, &other).unwrap()),
        permission: Permission::Read,
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
    let r = h
        .call(
            Method::POST,
            "/api/links",
            Some(&alice.token),
            Some(CreateLinkRequest {
                node_id: other.clone(),
                password: Some("letmein".into()),
                expires_at: None,
            }),
        )
        .await;
    assert_eq!(r.status, StatusCode::CREATED);
    let link: Link = r.json();
    assert!(link.has_password);
    // What the browser would have: /s/<token>#<key>. Only the token is sent.
    let fragment_key = other_key.to_b64();
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
            Some(json!({"password": "nope"})),
        )
        .await;
    assert_eq!(r.status, StatusCode::UNAUTHORIZED);
    let r = h
        .call(
            Method::POST,
            &format!("{base}/unlock"),
            None,
            Some(json!({"password": "letmein"})),
        )
        .await;
    assert_eq!(r.status, StatusCode::OK);
    let unlocked: UnlockLinkResponse = r.json();
    let lt = [("x-link-token", unlocked.link_token.as_str())];
    let info: PublicLinkInfo = h
        .raw(Method::GET, &base, None, &lt, Body::empty(), None)
        .await
        .json();
    let lk = Key::from_b64(&fragment_key).unwrap();
    assert_eq!(
        c::decrypt_metadata(&lk, &info.node.id, &info.node.enc_metadata)
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
    // Precompressed copies are used when the browser accepts gzip.
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
                wrapped_key: B64(c::seal_share_key(&pk.public_key, &key, &f.id).unwrap()),
                permission: Permission::Read,
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

#[tokio::test]
async fn full_quota_prunes_old_versions_first() {
    // Each 1000-byte upload is 1040 bytes of ciphertext.
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
            wrapped_key: B64(c::seal_share_key(&pk.public_key, &folder_key, &folder).unwrap()),
            permission: Permission::Write,
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
                password: None,
                expires_at: None,
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
case " $* " in *" --dump-single-json "*)
  echo '{"title":"A test clip","extractor_key":"Youtube","uploader":"Someone","duration":12.5,"formats":[{"format_id":"140","ext":"m4a","vcodec":"none","acodec":"mp4a","filesize":900,"protocol":"https"},{"format_id":"134","ext":"mp4","vcodec":"avc1","acodec":"none","height":240,"filesize":2000,"protocol":"https"},{"format_id":"18","ext":"mp4","vcodec":"avc1","acodec":"mp4a","height":360,"filesize":3000,"protocol":"https"},{"format_id":"9999","ext":"mp4","vcodec":"avc1","acodec":"mp4a","height":2160,"protocol":"https"}]}'
  exit 0;;
esac
case "$url" in *big*) head -c 5000 /dev/zero; exit 0;; esac
case "$url" in *slow*) printf 'FAKE'; sleep 3; printf 'VIDEO'; exit 0;; esac
case " $* " in
  *" -f 140 "*) printf 'AUDIO-ONLY-BYTES'; exit 0;;
  *" -f 134 "*) printf 'VIDEO-ONLY-BYTES'; exit 0;;
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

    // Scratch directories are gone.
    let mut files = Vec::new();
    all_files(&h.dir.path().join("data/downloads"), &mut files);
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
