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
    };
    let r = h
        .call(Method::POST, "/api/auth/register", None, Some(&req))
        .await;
    assert_eq!(
        r.status,
        StatusCode::CREATED,
        "{}",
        String::from_utf8_lossy(&r.body)
    );
    let s: SessionResponse = r.json();
    Client {
        username: username.into(),
        token: s.token,
        mk,
        kp,
        root: root_id,
    }
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

    // --- new version replaces old content; quota tracks it ------------------
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
    assert!(alice.me(&h).await.used_bytes < used_before);
    let blob_dir = h
        .dir
        .path()
        .join("data/blobs")
        .join(&old_vid[..2])
        .join(&old_vid);
    assert!(!blob_dir.exists(), "old version blobs are deleted");
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
}
