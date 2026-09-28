//! The CLI client and the FUSE mount against an in-process server over
//! real TCP. Also checks that no plaintext reaches the database or blobs.

use std::fs;
use std::io::{ErrorKind, Read, Write};
use std::net::SocketAddr;
use std::path::Path;

use serde::Serialize;
use serde::de::DeserializeOwned;
use thencloud_cli::{Client, Entry};
use thencloud_crypto::api::*;
use thencloud_crypto::{self as c, KdfParams, Key, KeyPair, Metadata};
use thencloud_server::{AppState, Config, router};

const FAST_KDF: KdfParams = KdfParams {
    m_cost: 19 * 1024,
    t_cost: 2,
    p_cost: 1,
};
const MARKER: &[u8] = b"CLI-SECRET-PAYLOAD-";

struct Server {
    url: String,
    dir: tempfile::TempDir,
    token: String,
    app_password: Key,
    read_only_password: Key,
}

fn call<B: Serialize, T: DeserializeOwned>(url: &str, token: Option<&str>, body: &B) -> T {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .http_status_as_error(false)
        .build()
        .into();
    let mut req = agent.post(url);
    if let Some(t) = token {
        req = req.header("Authorization", format!("Bearer {t}"));
    }
    let mut res = req.send_json(body).unwrap();
    let status = res.status();
    let text = res.body_mut().read_to_string().unwrap();
    assert!(status.is_success(), "{url}: {status} {text}");
    serde_json::from_str(&text).unwrap()
}

fn get<T: DeserializeOwned>(s: &Server, path: &str) -> T {
    let mut res = ureq::get(format!("{}{path}", s.url))
        .header("Authorization", format!("Bearer {}", s.token))
        .call()
        .unwrap();
    res.body_mut().read_json().unwrap()
}

fn start() -> Server {
    let dir = tempfile::tempdir().unwrap();
    let (tx, rx) = std::sync::mpsc::channel();
    let cfg = Config::for_dir(dir.path());
    std::thread::spawn(move || {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async move {
            let state = AppState::new(cfg).await.unwrap();
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            tx.send(listener.local_addr().unwrap()).unwrap();
            axum::serve(
                listener,
                router(state).into_make_service_with_connect_info::<SocketAddr>(),
            )
            .await
            .unwrap();
        });
    });
    let url = format!("http://{}", rx.recv().unwrap());

    let password = "correct horse battery staple";
    let salt = c::random_bytes(c::SALT_LEN);
    let ak = c::derive_account_keys(password, &salt, FAST_KDF).unwrap();
    let mk = Key::generate();
    let kp = KeyPair::generate();
    let root_id = c::new_id();
    let root_key = Key::generate();
    let root_meta = Metadata {
        name: "root".into(),
        mime: None,
        size: 0,
        mtime: 0,
        changed: None,
    };
    let session: SessionResponse = call(
        &format!("{url}/api/auth/register"),
        None,
        &RegisterRequest {
            username: "alice".into(),
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
                enc_metadata: B64(c::encrypt_metadata(&root_key, &root_id, &root_meta).unwrap()),
            },
            device_name: Some("test".into()),
            invite: None,
        },
    );
    let app_password = |scope| {
        let secret = Key::generate();
        let keys = c::derive_app_password_keys(&secret);
        let id = c::new_id();
        let _: AppPassword = call(
            &format!("{url}/api/app-passwords"),
            Some(&session.token),
            &CreateAppPasswordRequest {
                id: id.clone(),
                name: "cli".into(),
                scope,
                current_auth_key: B64(ak.auth_key.as_bytes().to_vec()),
                auth_key: B64(keys.auth_key.as_bytes().to_vec()),
                enc_master_key: B64(c::wrap_master_key_app(&keys.kek, &mk, &id)),
            },
        );
        secret
    };
    let full = app_password(AppScope::Full);
    let read = app_password(AppScope::Read);
    Server {
        url,
        dir,
        token: session.token,
        app_password: full,
        read_only_password: read,
    }
}

fn client(s: &Server) -> Client {
    Client::login(&s.url, &s.app_password, "test").unwrap()
}

/// Every byte the server stored, database and blobs alike.
fn assert_no_plaintext(s: &Server, needles: &[&[u8]]) {
    fn walk(p: &Path, out: &mut Vec<(String, Vec<u8>)>) {
        for e in fs::read_dir(p).unwrap().flatten() {
            let path = e.path();
            if path.is_dir() {
                walk(&path, out);
            } else if let Ok(b) = fs::read(&path) {
                out.push((path.display().to_string(), b));
            }
        }
    }
    let mut files = Vec::new();
    walk(s.dir.path(), &mut files);
    assert!(!files.is_empty());
    for (path, bytes) in files {
        for n in needles {
            assert!(
                !bytes.windows(n.len()).any(|w| w == *n),
                "plaintext {:?} in {path}",
                String::from_utf8_lossy(n)
            );
        }
    }
}

fn content(len: usize, seed: u8) -> Vec<u8> {
    let mut v = MARKER.to_vec();
    v.extend((0..len).map(|i| (i as u8).wrapping_mul(31).wrapping_add(seed)));
    v.truncate(len.max(MARKER.len()));
    v
}

fn child(cl: &Client, folder: &Entry, name: &str) -> Option<Entry> {
    cl.list(folder)
        .unwrap()
        .into_iter()
        .find(|e| e.meta.name == name)
}

fn fetch(cl: &Client, e: &Entry) -> Vec<u8> {
    let mut out = Vec::new();
    cl.download(e, &mut out).unwrap();
    out
}

#[test]
fn upload_download_push_and_pull() {
    let s = start();
    let cl = client(&s);
    let root = cl.root().unwrap();
    let docs = cl.mkdir(&root, "Quarterly secrets").unwrap();

    let local = tempfile::tempdir().unwrap();
    let big = content(9 * 1024 * 1024 + 123, 1);
    fs::write(local.path().join("big.bin"), &big).unwrap();
    let up = cl
        .upload(&local.path().join("big.bin"), &docs, "big.bin", None)
        .unwrap();
    assert_eq!(fetch(&cl, &up), big);
    assert_eq!(
        cl.resolve("quarterly secrets/BIG.bin").unwrap().node.id,
        up.node.id
    );

    // A second upload is a new version of the same node.
    let small = content(10, 2);
    fs::write(local.path().join("big.bin"), &small).unwrap();
    let v2 = cl
        .upload(&local.path().join("big.bin"), &docs, "big.bin", Some(&up))
        .unwrap();
    assert_eq!(v2.node.id, up.node.id);
    assert_eq!(fetch(&cl, &child(&cl, &docs, "big.bin").unwrap()), small);

    let tree = tempfile::tempdir().unwrap();
    fs::create_dir_all(tree.path().join("a/b")).unwrap();
    fs::write(tree.path().join("a/b/deep.txt"), content(100, 3)).unwrap();
    fs::write(tree.path().join("top.txt"), content(50, 4)).unwrap();
    let pushed = cl.push(tree.path(), &docs, &mut |_| {}).unwrap();
    assert_eq!(pushed.transferred, 2);
    assert_eq!(
        cl.push(tree.path(), &docs, &mut |_| {})
            .unwrap()
            .transferred,
        0
    );

    let back = tempfile::tempdir().unwrap();
    let pulled = cl.pull(&docs, back.path(), &mut |_| {}).unwrap();
    assert_eq!(pulled.transferred, 3);
    assert_eq!(
        fs::read(back.path().join("a/b/deep.txt")).unwrap(),
        content(100, 3)
    );
    assert_eq!(
        cl.pull(&docs, back.path(), &mut |_| {})
            .unwrap()
            .transferred,
        0
    );

    cl.logout().unwrap();
    assert_no_plaintext(&s, &[MARKER, b"Quarterly secrets", b"deep.txt"]);
}

#[test]
fn backup_restores_to_another_server() {
    let (a, b) = (start(), start());
    let (ca, cb) = (client(&a), client(&b));
    let root = ca.root().unwrap();
    let docs = ca.mkdir(&root, "Quarterly secrets").unwrap();
    let deep = ca.mkdir(&ca.mkdir(&docs, "a").unwrap(), "b").unwrap();
    ca.mkdir(&docs, "empty").unwrap();
    // Bigger than a data record, so a file spans several.
    let big = content(9 * 1024 * 1024 + 7, 5);
    let small = content(100, 6);
    ca.upload_from(
        &mut &big[..],
        big.len() as u64,
        1_700_000_000_000,
        &docs,
        "big.bin",
        None,
    )
    .unwrap();
    ca.upload_from(
        &mut &small[..],
        small.len() as u64,
        0,
        &deep,
        "deep.txt",
        None,
    )
    .unwrap();
    ca.upload_from(&mut &b""[..], 0, 0, &deep, "nothing.txt", None)
        .unwrap();

    let key = Key::generate();
    let mut file = Vec::new();
    let s = ca
        .backup(&docs, &mut file, key.clone(), &mut |_| {})
        .unwrap();
    assert_eq!(s.transferred, 3);
    assert!(
        !file.windows(MARKER.len()).any(|w| w == MARKER),
        "the backup is sealed"
    );
    assert!(!file.windows(8).any(|w| w == b"deep.txt"));

    // Restored on the other server, twice: the second time as new versions.
    let target = cb.mkdir(&cb.root().unwrap(), "From A").unwrap();
    for _ in 0..2 {
        let s = cb
            .restore(&file[..], key.clone(), &target, &mut |_| {})
            .unwrap();
        assert_eq!(s.transferred, 3);
    }
    let got = cb.resolve("From A/big.bin").unwrap();
    assert_eq!(fetch(&cb, &got), big);
    assert_eq!(got.meta.mtime, 1_700_000_000_000);
    assert_eq!(
        fetch(&cb, &cb.resolve("From A/a/b/deep.txt").unwrap()),
        small
    );
    assert_eq!(
        fetch(&cb, &cb.resolve("From A/a/b/nothing.txt").unwrap()),
        b""
    );
    assert!(cb.resolve("From A/empty").unwrap().is_folder());
    assert_eq!(
        cb.list(&target).unwrap().len(),
        3,
        "folders reused, not doubled"
    );

    // The wrong key, or a backup cut short, restores nothing more.
    let other = cb.mkdir(&cb.root().unwrap(), "Broken").unwrap();
    let err = cb
        .restore(&file[..], Key::generate(), &other, &mut |_| {})
        .err()
        .unwrap();
    assert!(err.to_string().contains("doesn't open"), "{err}");
    let err = cb
        .restore(&file[..file.len() - 40], key.clone(), &other, &mut |_| {})
        .err()
        .unwrap();
    assert!(err.to_string().contains("damaged"), "{err}");

    ca.logout().unwrap();
    cb.logout().unwrap();
    assert_no_plaintext(&b, &[MARKER, b"Quarterly secrets", b"deep.txt", b"From A"]);
}

fn fuse_available() -> bool {
    let ok = Path::new("/dev/fuse").exists()
        && std::env::var_os("PATH").is_some_and(|p| {
            std::env::split_paths(&p)
                .any(|d| d.join("fusermount3").exists() || d.join("fusermount").exists())
        });
    if !ok {
        eprintln!("skipping: FUSE isn't available here");
    }
    ok
}

fn mount_at(
    s: &Server,
    password: &Key,
    read_only: bool,
) -> (tempfile::TempDir, fuser::BackgroundSession) {
    use thencloud_cli::mount::{MountOptions, spawn};
    let cl = Client::login(&s.url, password, "mount").unwrap();
    let root = cl.root().unwrap();
    let mp = tempfile::tempdir().unwrap();
    let opts = MountOptions {
        read_only,
        allow_other: false,
        temp_dir: std::env::temp_dir(),
    };
    let bg = spawn(cl, root, mp.path(), opts).unwrap();
    (mp, bg)
}

#[test]
fn mount_reads_writes_and_renames() {
    if !fuse_available() {
        return;
    }
    let s = start();
    let cl = client(&s);
    let root = cl.root().unwrap();

    // Made through the API before mounting, read through the mount.
    let local = tempfile::tempdir().unwrap();
    let big = content(9 * 1024 * 1024 + 7, 5);
    fs::write(local.path().join("film.bin"), &big).unwrap();
    let film = cl
        .upload(&local.path().join("film.bin"), &root, "film.bin", None)
        .unwrap();

    let (mp, bg) = mount_at(&s, &s.app_password, false);
    let m = mp.path();
    assert_eq!(fs::read(m.join("film.bin")).unwrap(), big);
    // A read in the middle, across a chunk boundary.
    let mut f = fs::File::open(m.join("film.bin")).unwrap();
    std::io::Seek::seek(&mut f, std::io::SeekFrom::Start(4 * 1024 * 1024 - 10)).unwrap();
    let mut buf = [0u8; 20];
    f.read_exact(&mut buf).unwrap();
    assert_eq!(&buf[..], &big[4 * 1024 * 1024 - 10..4 * 1024 * 1024 + 10]);
    drop(f);

    // New files and folders land on the server, encrypted.
    fs::create_dir(m.join("Taxes 2026")).unwrap();
    let note = content(3000, 6);
    fs::write(m.join("Taxes 2026/receipt.txt"), &note).unwrap();
    fs::write(m.join("empty"), b"").unwrap();
    let taxes = child(&cl, &root, "Taxes 2026").unwrap();
    let receipt = child(&cl, &taxes, "receipt.txt").unwrap();
    assert_eq!(fetch(&cl, &receipt), note);
    assert_eq!(child(&cl, &root, "empty").unwrap().meta.size, 0);
    let mut names: Vec<_> = fs::read_dir(m)
        .unwrap()
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .collect();
    names.sort();
    assert_eq!(names, ["Taxes 2026", "empty", "film.bin"]);

    // Appending uploads a new version with the whole file.
    fs::OpenOptions::new()
        .append(true)
        .open(m.join("Taxes 2026/receipt.txt"))
        .unwrap()
        .write_all(b" paid")
        .unwrap();
    let mut both = note.clone();
    both.extend_from_slice(b" paid");
    assert_eq!(
        fetch(&cl, &child(&cl, &taxes, "receipt.txt").unwrap()),
        both
    );
    assert_eq!(fs::read(m.join("Taxes 2026/receipt.txt")).unwrap(), both);

    // Editors save by writing a temporary file and renaming it over the
    // original: that must become a new version of the same file, and the
    // temporary file must not linger in the trash.
    fs::write(m.join("Taxes 2026/.receipt.txt.swp"), b"rewritten").unwrap();
    fs::rename(
        m.join("Taxes 2026/.receipt.txt.swp"),
        m.join("Taxes 2026/receipt.txt"),
    )
    .unwrap();
    assert_eq!(
        fs::read(m.join("Taxes 2026/receipt.txt")).unwrap(),
        b"rewritten"
    );
    let saved = child(&cl, &taxes, "receipt.txt").unwrap();
    assert_eq!(saved.node.id, receipt.node.id);
    assert_eq!(fetch(&cl, &saved), b"rewritten");
    let versions: Vec<FileVersion> = get(&s, &format!("/api/nodes/{}/versions", receipt.node.id));
    assert_eq!(versions.len(), 3);
    let trash: Vec<TrashItem> = get(&s, "/api/trash");
    assert!(trash.is_empty());
    assert!(child(&cl, &taxes, ".receipt.txt.swp").is_none());

    // Truncating on open.
    fs::write(m.join("film.bin"), b"short").unwrap();
    assert_eq!(
        fetch(&cl, &child(&cl, &root, "film.bin").unwrap()),
        b"short"
    );
    assert_eq!(fs::metadata(m.join("film.bin")).unwrap().len(), 5);

    // Rename and move keep the node; the key is re-wrapped for the new folder.
    fs::rename(m.join("film.bin"), m.join("Taxes 2026/clip.bin")).unwrap();
    let moved = child(&cl, &taxes, "clip.bin").unwrap();
    assert_eq!(moved.node.id, film.node.id);
    assert_eq!(fetch(&cl, &moved), b"short");
    assert!(child(&cl, &root, "film.bin").is_none());

    // Names are unique regardless of case.
    fs::write(m.join("Readme"), b"x").unwrap();
    let e = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(m.join("README"))
        .unwrap_err();
    assert_eq!(e.kind(), ErrorKind::AlreadyExists);

    // Removing goes to the trash, and folders must be empty first.
    let e = fs::remove_dir(m.join("Taxes 2026")).unwrap_err();
    assert_eq!(e.raw_os_error(), Some(libc::ENOTEMPTY));
    fs::remove_file(m.join("Taxes 2026/clip.bin")).unwrap();
    let trash: Vec<TrashItem> = get(&s, "/api/trash");
    assert_eq!(trash.len(), 1);
    assert_eq!(trash[0].node.id, film.node.id);

    // A file deleted while still open is never uploaded.
    let mut tmp = fs::File::create(m.join("scratch")).unwrap();
    fs::remove_file(m.join("scratch")).unwrap();
    tmp.write_all(b"gone").unwrap();
    drop(tmp);
    assert!(child(&cl, &root, "scratch").is_none());

    // Changes made elsewhere show up when the folder is listed again.
    let other = cl.mkdir(&root, "From the browser").unwrap();
    fs::write(local.path().join("hi.txt"), b"hello").unwrap();
    cl.upload(&local.path().join("hi.txt"), &other, "hi.txt", None)
        .unwrap();
    assert!(
        fs::read_dir(m)
            .unwrap()
            .any(|e| e.unwrap().file_name() == "From the browser")
    );
    assert_eq!(
        fs::read(m.join("From the browser/hi.txt")).unwrap(),
        b"hello"
    );

    // Space is the account's quota.
    let st = nix_statvfs(m);
    assert!(st > 0);

    drop(bg);
    cl.logout().unwrap();
    assert_no_plaintext(
        &s,
        &[
            MARKER,
            b"Taxes 2026",
            b"receipt.txt",
            b"rewritten",
            b"clip.bin",
        ],
    );
}

#[test]
fn mount_with_read_only_password_refuses_changes() {
    if !fuse_available() {
        return;
    }
    let s = start();
    let cl = client(&s);
    let root = cl.root().unwrap();
    let local = tempfile::tempdir().unwrap();
    fs::write(local.path().join("a.txt"), b"keep me").unwrap();
    cl.upload(&local.path().join("a.txt"), &root, "a.txt", None)
        .unwrap();

    let (mp, bg) = mount_at(&s, &s.read_only_password, false);
    let m = mp.path();
    assert_eq!(fs::read(m.join("a.txt")).unwrap(), b"keep me");
    for e in [
        fs::write(m.join("a.txt"), b"x").unwrap_err(),
        fs::write(m.join("b.txt"), b"x").unwrap_err(),
        fs::create_dir(m.join("dir")).unwrap_err(),
        fs::remove_file(m.join("a.txt")).unwrap_err(),
    ] {
        assert_eq!(e.raw_os_error(), Some(libc::EROFS), "{e}");
    }
    drop(bg);
    assert_eq!(fetch(&cl, &child(&cl, &root, "a.txt").unwrap()), b"keep me");
}

fn nix_statvfs(p: &Path) -> u64 {
    let path = std::ffi::CString::new(p.as_os_str().as_encoded_bytes()).unwrap();
    let mut st: libc::statvfs = unsafe { std::mem::zeroed() };
    assert_eq!(unsafe { libc::statvfs(path.as_ptr(), &mut st) }, 0);
    st.f_blocks * st.f_frsize
}

#[test]
fn verify_web_checks_signature_and_every_encoding() {
    use base64::Engine as _;
    use base64::engine::general_purpose::STANDARD;
    use blake2::{Blake2b512, Digest as _};
    use ring::signature::{Ed25519KeyPair, KeyPair as _};
    use sha2::Sha256;
    use thencloud_cli::verify::{Manifest, verify_minisign, verify_web};

    let s = start();
    let web = s.dir.path().join("web");
    let app = "console.log('the real client');".repeat(100);
    let files = [
        (
            "index.html",
            "<!doctype html><script type=module src=/assets/app.js></script>".to_string(),
        ),
        (
            "share.html",
            "<!doctype html><title>share</title>".to_string(),
        ),
        ("assets/app.js", app.clone()),
        ("sw.js", "self.onfetch = () => {};".to_string()),
    ];
    let gz = |text: &str| {
        let mut e = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::best());
        e.write_all(text.as_bytes()).unwrap();
        e.finish().unwrap()
    };
    let mut hashes = serde_json::Map::new();
    for (path, body) in &files {
        let p = web.join(path);
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(&p, body).unwrap();
        let h: String = Sha256::digest(body.as_bytes())
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        hashes.insert(path.to_string(), h.into());
    }
    fs::write(web.join("assets/app.js.gz"), gz(&app)).unwrap();
    let manifest = serde_json::to_vec_pretty(
        &serde_json::json!({"name": "thencloud-web", "version": "test", "files": hashes}),
    )
    .unwrap();

    // Signed the way `minisign -S` does (prehashed with BLAKE2b-512).
    let rng = ring::rand::SystemRandom::new();
    let kp =
        Ed25519KeyPair::from_pkcs8(Ed25519KeyPair::generate_pkcs8(&rng).unwrap().as_ref()).unwrap();
    let key_id = [7u8, 1, 2, 3, 4, 5, 6, 8];
    let public = STANDARD.encode([b"Ed".as_slice(), &key_id, kp.public_key().as_ref()].concat());
    let sign = |msg: &[u8], comment: &str| {
        let sig = kp.sign(&Blake2b512::digest(msg));
        let global = kp.sign(&[sig.as_ref(), comment.as_bytes()].concat());
        format!(
            "untrusted comment: signature from minisign secret key\n{}\ntrusted comment: {comment}\n{}\n",
            STANDARD.encode([b"ED".as_slice(), &key_id, sig.as_ref()].concat()),
            STANDARD.encode(global.as_ref()),
        )
    };
    let sig = sign(&manifest, "thencloud-web test");
    let pub_file = format!("untrusted comment: minisign public key\n{public}\n");
    assert_eq!(
        verify_minisign(&manifest, &sig, &pub_file).unwrap(),
        "thencloud-web test"
    );
    let mut changed = manifest.clone();
    changed[20] ^= 1;
    assert!(verify_minisign(&changed, &sig, &public).is_err());
    let forged_comment = sig.replace(
        "trusted comment: thencloud-web test",
        "trusted comment: v9.9",
    );
    assert!(verify_minisign(&manifest, &forged_comment, &public).is_err());
    let other = STANDARD.encode([b"Ed".as_slice(), &key_id, &[9u8; 32]].concat());
    assert!(verify_minisign(&manifest, &sig, &other).is_err());

    // The real files pass, in every encoding, and so do `/` and share links.
    let m = Manifest::parse(&manifest).unwrap();
    let r = verify_web(&s.url, &m).unwrap();
    assert!(r.problems.is_empty(), "{:?}", r.problems);
    assert_eq!(r.checked, (files.len() + 2) * 3);

    // A swapped gzip copy is caught, though the plain file is untouched.
    fs::write(web.join("assets/app.js.gz"), gz("steal(location.hash)")).unwrap();
    fs::remove_file(web.join("sw.js")).unwrap();
    let r = verify_web(&s.url, &m).unwrap();
    assert_eq!(r.problems.len(), 4, "{:?}", r.problems);
    assert!(r.problems[0].starts_with("/assets/app.js (gzip): different content"));
    assert!(
        r.problems[1..]
            .iter()
            .all(|p| p.starts_with("/sw.js") && p.ends_with("HTTP 404"))
    );
}
