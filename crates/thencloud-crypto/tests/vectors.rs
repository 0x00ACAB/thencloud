//! Test vectors for every format in docs/format/README.md.
//!
//! `vectors_match` checks docs/format/vectors.json against this crate. The
//! same file is checked against the WASM build by web/tests/vectors.test.js,
//! and any other client can check itself against it too.
//!
//! Ciphertexts use random nonces, so they're checked by opening them: each
//! one is fixed in the file, and must open to the given plaintext under the
//! given key and context, or (with `"valid": false`) must not open at all.
//! Everything deterministic (key derivation, tags, fingerprints, padding) is
//! compared exactly.
//!
//! thencloud holds people's data, so the file only grows: what's in it was
//! written by an earlier build and must keep opening. To add vectors for a
//! new format (see `merge`):
//!
//!     THENCLOUD_WRITE_VECTORS=1 cargo test -p thencloud-crypto --test vectors -- --ignored
//!
//! A format never changes in place. A new layout gets a new version or kind
//! byte (docs/format/README.md, "Versions"), and the old vectors stay.

use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use thencloud_crypto::*;

const PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../docs/format/vectors.json"
);

/// Fixed, random-looking test bytes, named so the file says where they came from.
fn bytes(label: &str, n: usize) -> Vec<u8> {
    let mut out = Vec::new();
    let mut i = 0u32;
    while out.len() < n {
        out.extend_from_slice(&Sha256::digest(format!(
            "thencloud test vector/{label}/{i}"
        )));
        i += 1;
    }
    out.truncate(n);
    out
}

fn key(label: &str) -> Key {
    Key::from_slice(&bytes(label, KEY_LEN)).unwrap()
}

/// The associated data every format binds, as the spec describes it.
fn aad(label: &str, parts: &[&str]) -> Vec<u8> {
    let mut v = format!("thencloud/v1/{label}").into_bytes();
    for p in parts {
        v.push(0);
        v.extend_from_slice(p.as_bytes());
    }
    v
}

fn b64(v: &[u8]) -> Value {
    json!(b64_encode(v))
}

fn get<'a>(v: &'a Value, field: &str) -> &'a Value {
    v.get(field)
        .unwrap_or_else(|| panic!("missing {field} in {v}"))
}

fn text<'a>(v: &'a Value, field: &str) -> &'a str {
    get(v, field).as_str().unwrap()
}

fn raw(v: &Value, field: &str) -> Vec<u8> {
    b64_decode(text(v, field)).unwrap()
}

fn context(v: &Value) -> Vec<String> {
    get(v, "context")
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s.as_str().unwrap().to_owned())
        .collect()
}

// ---------------------------------------------------------------------------
// Checking
// ---------------------------------------------------------------------------

/// Opens a symmetric vector through this crate's public API, the way a
/// client would.
fn open_symmetric(v: &Value) -> Result<Vec<u8>> {
    let k = Key::from_slice(&raw(v, "key"))?;
    let ctx = context(v);
    let c: Vec<&str> = ctx.iter().map(String::as_str).collect();
    let sealed = raw(v, "sealed");
    let bytes = |k: Key| k.as_bytes().to_vec();
    Ok(match text(v, "format") {
        "master-key" => bytes(unwrap_master_key(&k, &sealed)?),
        "master-key-recovery" => bytes(unwrap_master_key_recovery(&k, &sealed)?),
        "master-key-app" => bytes(unwrap_master_key_app(&k, &sealed, c[0])?),
        "master-key-passkey" => bytes(unwrap_master_key_passkey(&k, &sealed, &b64_decode(c[0])?)?),
        "private-key" => bytes(unwrap_private_key(&k, &sealed)?.secret),
        "pq-private-key" => unwrap_pq_private_key(&k, &sealed)?.seed().to_vec(),
        "private-data" => decrypt_private_data(&k, c[0], c[1], &sealed)?,
        "avatar" => decrypt_avatar(&k, c[0], &sealed)?,
        "person-details" => {
            let d = decrypt_person_details(&k, c[0], &sealed)?;
            assert_eq!(
                serde_json::to_value(&d).unwrap(),
                *get(v, "details"),
                "person details"
            );
            let mut b = serde_json::to_vec(&d).unwrap();
            b.resize(PERSON_DETAILS_PADDED, 0);
            b
        }
        "display-name" => {
            let name = decrypt_display_name(&k, c[0], &sealed)?;
            assert_eq!(name, text(v, "name"), "display name");
            let mut b = name.into_bytes();
            b.resize(DISPLAY_NAME_PADDED, 0);
            b
        }
        "node-key" => bytes(unwrap_node_key(&k, &sealed, c[0])?),
        "metadata" => {
            let meta = decrypt_metadata(&k, c[0], &sealed)?;
            assert_eq!(
                serde_json::to_value(&meta).unwrap(),
                *get(v, "metadata"),
                "metadata fields"
            );
            open(&k, &sealed, &aad("metadata", &c))?
        }
        "content-key" => bytes(unwrap_content_key(&k, &sealed, c[0], c[1])?),
        "link-key" => bytes(unwrap_link_key(&k, &sealed, c[0])?),
        "link-secret" => bytes(decrypt_link_secret(&k, &sealed, c[0])?),
        "thumbnail" => decrypt_thumbnail(&k, c[0], c[1], &sealed)?,
        "comment" => decrypt_comment(&k, c[0], c[1], c[2], &sealed)?,
        "backup" => open_backup_record(&k, &b64_decode(c[0])?, c[1].parse().unwrap(), &sealed)?,
        "chunk" => decrypt_chunk(&k, c[0], c[1].parse().unwrap(), c[2] == "last", &sealed)?,
        f => panic!("unknown format {f}"),
    })
}

fn open_box(v: &Value) -> Result<Vec<u8>> {
    let secret = raw(v, "secret");
    let mut kp = KeyPair::from_secret(Key::from_slice(&secret[..KEY_LEN])?);
    if secret.len() > KEY_LEN {
        kp = kp.with_pq(PqKeyPair::from_seed(&secret[KEY_LEN..])?);
    }
    let ctx = context(v);
    let sealed = raw(v, "sealed");
    let k = match text(v, "format") {
        "share" => open_share_key(&kp, &sealed, &ctx[0])?,
        "drop" => open_drop_key(&kp, &sealed, &ctx[0], &ctx[1])?,
        "avatar-key" => open_avatar_key(&kp, &sealed, &ctx[0], &ctx[1])?,
        f => panic!("unknown format {f}"),
    };
    Ok(k.as_bytes().to_vec())
}

fn check_ciphertexts(list: &Value, open_one: fn(&Value) -> Result<Vec<u8>>) {
    for v in list.as_array().unwrap() {
        let valid = v.get("valid").and_then(Value::as_bool).unwrap_or(true);
        let got = open_one(v);
        if valid {
            let got = got.unwrap_or_else(|e| panic!("{e}: {v}"));
            assert_eq!(got, raw(v, "plaintext"), "{v}");
            // The documented associated data is what's actually bound.
            let ctx = context(v);
            let c: Vec<&str> = ctx.iter().map(String::as_str).collect();
            assert_eq!(raw(v, "aad"), aad(text(v, "format"), &c), "{v}");
        } else {
            assert!(got.is_err(), "should not open: {v}");
        }
    }
}

#[test]
fn vectors_match() {
    let file: Value =
        serde_json::from_str(&std::fs::read_to_string(PATH).expect("docs/format/vectors.json"))
            .unwrap();

    for v in get(&file, "account_keys").as_array().unwrap() {
        let p = get(v, "params");
        let params = KdfParams {
            m_cost: get(p, "m_cost").as_u64().unwrap() as u32,
            t_cost: get(p, "t_cost").as_u64().unwrap() as u32,
            p_cost: get(p, "p_cost").as_u64().unwrap() as u32,
        };
        let k = derive_account_keys(text(v, "password"), &raw(v, "salt"), params).unwrap();
        assert_eq!(k.auth_key.as_bytes().to_vec(), raw(v, "auth_key"), "{v}");
        assert_eq!(k.kek.as_bytes().to_vec(), raw(v, "kek"), "{v}");
    }

    for v in get(&file, "recovery_keys").as_array().unwrap() {
        let k = Key::from_slice(&raw(v, "key")).unwrap();
        assert_eq!(encode_recovery_key(&k), text(v, "text"));
        for typed in get(v, "also_accepted").as_array().unwrap() {
            assert!(decode_recovery_key(typed.as_str().unwrap()).unwrap() == k);
        }
        let d = derive_recovery_keys(&k);
        assert_eq!(d.auth_key.as_bytes().to_vec(), raw(v, "auth_key"));
        assert_eq!(d.kek.as_bytes().to_vec(), raw(v, "kek"));
    }

    for v in get(&file, "app_password_keys").as_array().unwrap() {
        let d = derive_app_password_keys(&Key::from_slice(&raw(v, "key")).unwrap());
        assert_eq!(d.auth_key.as_bytes().to_vec(), raw(v, "auth_key"));
        assert_eq!(d.kek.as_bytes().to_vec(), raw(v, "kek"));
    }

    for v in get(&file, "passkeys").as_array().unwrap() {
        assert_eq!(passkey_prf_salt().to_vec(), raw(v, "prf_salt"));
        let kek = derive_passkey_kek(&raw(v, "prf_output")).unwrap();
        assert_eq!(kek.as_bytes().to_vec(), raw(v, "kek"));
    }

    for v in get(&file, "link_passwords").as_array().unwrap() {
        let secret = raw(v, "secret");
        assert_eq!(link_password_salt(&secret).to_vec(), raw(v, "salt"));
        let d = derive_link_password_keys(&secret, text(v, "password")).unwrap();
        assert_eq!(d.auth_key.as_bytes().to_vec(), raw(v, "auth_key"), "{v}");
        assert_eq!(d.kek.as_bytes().to_vec(), raw(v, "kek"), "{v}");
    }

    for v in get(&file, "keypairs").as_array().unwrap() {
        let kp = KeyPair::from_secret(Key::from_slice(&raw(v, "x25519_secret")).unwrap());
        assert_eq!(kp.public.to_vec(), raw(v, "x25519_public"));
        let pq_public = match v.get("pq_seed") {
            Some(_) => {
                let pq = PqKeyPair::from_seed(&raw(v, "pq_seed")).unwrap();
                assert_eq!(pq.public, raw(v, "pq_public"));
                Some(pq.public.clone())
            }
            None => None,
        };
        let id = identity(&kp.public, pq_public.as_deref());
        assert_eq!(id, raw(v, "identity"));
        assert_eq!(fingerprint(&id), text(v, "fingerprint"));
    }

    for v in get(&file, "name_tags").as_array().unwrap() {
        let k = Key::from_slice(&raw(v, "folder_key")).unwrap();
        assert_eq!(name_tag(&k, text(v, "name")), raw(v, "tag"), "{v}");
    }

    for v in get(&file, "padding").as_array().unwrap() {
        let size = get(v, "size").as_u64().unwrap();
        let padded = get(v, "padded").as_u64().unwrap();
        assert_eq!(padded_size(size), padded, "{v}");
        assert_eq!(
            chunk_count(padded) as u64,
            get(v, "chunks").as_u64().unwrap()
        );
    }

    check_ciphertexts(get(&file, "symmetric"), open_symmetric);
    check_ciphertexts(get(&file, "sealed_boxes"), open_box);
}

// ---------------------------------------------------------------------------
// Writing
// ---------------------------------------------------------------------------

fn sym(format: &str, k: &Key, ctx: &[&str], plaintext: &[u8], sealed: Vec<u8>) -> Value {
    json!({
        "format": format,
        "key": b64(k.as_bytes()),
        "context": ctx,
        "aad": b64(&aad(format, ctx)),
        "plaintext": b64(plaintext),
        "sealed": b64(&sealed),
    })
}

/// A copy of a vector that must not open, with why.
fn reject(v: &Value, changes: Value, why: &str) -> Value {
    let mut v = v.clone();
    let o = v.as_object_mut().unwrap();
    for (k, x) in changes.as_object().unwrap() {
        o.insert(k.clone(), x.clone());
    }
    o.remove("aad");
    o.remove("plaintext");
    o.insert("valid".into(), json!(false));
    o.insert("why".into(), json!(why));
    v
}

#[test]
#[ignore = "writes docs/format/vectors.json; run with THENCLOUD_WRITE_VECTORS=1"]
fn write_vectors() {
    if std::env::var_os("THENCLOUD_WRITE_VECTORS").is_none() {
        return;
    }
    let fast = KdfParams {
        m_cost: 19 * 1024,
        t_cost: 2,
        p_cost: 1,
    };

    let mut account_keys = vec![];
    for (password, salt) in [
        ("correct horse battery staple", "salt-1"),
        ("p\u{e4}ssw\u{f6}rd", "salt-2"),
        // The same password with combining accents: the same keys.
        ("pa\u{308}sswo\u{308}rd", "salt-2"),
    ] {
        let salt = bytes(salt, SALT_LEN);
        let k = derive_account_keys(password, &salt, fast).unwrap();
        account_keys.push(json!({
            "password": password,
            "salt": b64(&salt),
            "params": fast,
            "auth_key": b64(k.auth_key.as_bytes()),
            "kek": b64(k.kek.as_bytes()),
        }));
    }

    let recovery = key("recovery key");
    let text = encode_recovery_key(&recovery);
    let loose = text.to_lowercase().replace('-', " ").replace('0', "o");
    let d = derive_recovery_keys(&recovery);
    let recovery_kek = d.kek.clone();
    let recovery_keys = vec![json!({
        "key": b64(recovery.as_bytes()),
        "text": text,
        "also_accepted": [text.replace('-', ""), loose],
        "auth_key": b64(d.auth_key.as_bytes()),
        "kek": b64(d.kek.as_bytes()),
    })];

    let app = key("app password");
    let d = derive_app_password_keys(&app);
    let app_kek = d.kek.clone();
    let app_password_keys = vec![json!({
        "key": b64(app.as_bytes()),
        "auth_key": b64(d.auth_key.as_bytes()),
        "kek": b64(d.kek.as_bytes()),
    })];

    let prf_output = bytes("passkey prf output", 32);
    let passkey_kek = derive_passkey_kek(&prf_output).unwrap();
    let passkeys = vec![json!({
        "prf_salt": b64(&passkey_prf_salt()),
        "prf_output": b64(&prf_output),
        "kek": b64(passkey_kek.as_bytes()),
    })];

    let link_secret = key("link secret");
    let link_keys = derive_link_password_keys(link_secret.as_bytes(), "open sesame").unwrap();
    let mut link_passwords = vec![];
    for (secret, password) in [
        (link_secret.as_bytes().to_vec(), "open sesame"),
        // A file drop's fragment: an owner's identity (64 bytes here).
        (bytes("file drop identity", 64), "drop it"),
    ] {
        let d = derive_link_password_keys(&secret, password).unwrap();
        link_passwords.push(json!({
            "secret": b64(&secret),
            "password": password,
            "salt": b64(&link_password_salt(&secret)),
            "auth_key": b64(d.auth_key.as_bytes()),
            "kek": b64(d.kek.as_bytes()),
        }));
    }

    let classic = KeyPair::from_secret(key("classic keypair"));
    let pq = PqKeyPair::from_seed(&bytes("hybrid keypair ml-kem seed", PQ_SEED_LEN)).unwrap();
    let hybrid = KeyPair::from_secret(key("hybrid keypair x25519")).with_pq(pq);
    let hybrid_pq = hybrid.pq.as_ref().unwrap();
    let classic_id = identity(&classic.public, None);
    let hybrid_id = identity(&hybrid.public, Some(&hybrid_pq.public));
    let keypairs = vec![
        json!({
            "x25519_secret": b64(classic.secret.as_bytes()),
            "x25519_public": b64(&classic.public),
            "identity": b64(&classic_id),
            "fingerprint": fingerprint(&classic_id),
        }),
        json!({
            "x25519_secret": b64(hybrid.secret.as_bytes()),
            "x25519_public": b64(&hybrid.public),
            "pq_seed": b64(hybrid_pq.seed()),
            "pq_public": b64(&hybrid_pq.public),
            "identity": b64(&hybrid_id),
            "fingerprint": fingerprint(&hybrid_id),
        }),
    ];

    let folder = key("folder key");
    let name_tags: Vec<Value> = [
        "Report.pdf",
        "report.PDF",
        "Caf\u{e9}.txt",
        "CAFE\u{301}.TXT",
        "Ünïcode ßtraße.txt",
        "notes",
    ]
    .iter()
    .map(|name| {
        json!({
            "folder_key": b64(folder.as_bytes()),
            "name": name,
            "tag": b64(&name_tag(&folder, name)),
        })
    })
    .collect();

    let padding: Vec<Value> = [
        0u64,
        1,
        256,
        257,
        1000,
        5000,
        16_383,
        16_384,
        16_385,
        20_000,
        123_456,
        4 * 1024 * 1024,
        4 * 1024 * 1024 + 1,
        9_999_999,
        3_000_000_000,
    ]
    .iter()
    .map(|&size| {
        let padded = padded_size(size);
        json!({ "size": size, "padded": padded, "chunks": chunk_count(padded) })
    })
    .collect();

    // Symmetric formats.
    let user = "0b1f5a9e-5d3c-4a8e-9f41-3c2d7e6b1a20";
    let node = "6f1c2b9a-3e4d-4f5a-8b6c-7d8e9f0a1b2c";
    let other_node = "9a8b7c6d-5e4f-4a3b-9c2d-1e0f9a8b7c6d";
    let version = "c3d4e5f6-a7b8-4c9d-8e0f-1a2b3c4d5e6f";
    let folder_id = "2e3f4a5b-6c7d-4e8f-9a0b-1c2d3e4f5a6b";
    let app_id = "5a6b7c8d-9e0f-4a1b-8c2d-3e4f5a6b7c8d";
    let credential_id = bytes("passkey credential id", 16);
    let credential_b64 = b64_encode(&credential_id);

    let kek = key("password kek");
    let mk = key("master key");
    let node_key = key("node key");
    let content_key = key("content key");
    let avatar_key = key("avatar key");
    let meta = Metadata {
        name: "Holiday photos.zip".into(),
        mime: Some("application/zip".into()),
        size: 1234,
        mtime: 1_790_000_000_000,
        changed: Some(1_790_000_123_456),
        taken: None,
    };
    let meta_sealed = encrypt_metadata(&node_key, node, &meta).unwrap();
    let meta_padded = open_raw(&node_key, &meta_sealed, &aad("metadata", &[node]));
    let old_meta = Metadata {
        name: "old.txt".into(),
        mime: None,
        size: 0,
        mtime: 0,
        changed: None,
        taken: None,
    };
    let old_meta_sealed = encrypt_metadata(&node_key, other_node, &old_meta).unwrap();
    let old_meta_padded = open_raw(&node_key, &old_meta_sealed, &aad("metadata", &[other_node]));
    let chunk0 = b"The first chunk of a file, padded with zeros in real use.".to_vec();
    let chunk1 = vec![0u8; 64];

    let mut symmetric = vec![
        sym(
            "master-key",
            &kek,
            &[],
            mk.as_bytes(),
            wrap_master_key(&kek, &mk),
        ),
        sym(
            "master-key-recovery",
            &recovery_kek,
            &[],
            mk.as_bytes(),
            wrap_master_key_recovery(&recovery_kek, &mk),
        ),
        sym(
            "master-key-app",
            &app_kek,
            &[app_id],
            mk.as_bytes(),
            wrap_master_key_app(&app_kek, &mk, app_id),
        ),
        {
            let mut v = sym(
                "master-key-passkey",
                &passkey_kek,
                &[&credential_b64],
                mk.as_bytes(),
                wrap_master_key_passkey(&passkey_kek, &mk, &credential_id),
            );
            v["prf_output"] = b64(&prf_output);
            v
        },
        sym(
            "private-key",
            &mk,
            &[],
            hybrid.secret.as_bytes(),
            wrap_private_key(&mk, &hybrid.secret),
        ),
        sym(
            "pq-private-key",
            &mk,
            &[],
            hybrid_pq.seed(),
            wrap_pq_private_key(&mk, hybrid_pq),
        ),
        sym(
            "private-data",
            &mk,
            &[user, "contacts"],
            br#"{"alice":{"public_key":"...","verified_at":0}}"#,
            encrypt_private_data(
                &mk,
                user,
                "contacts",
                br#"{"alice":{"public_key":"...","verified_at":0}}"#,
            ),
        ),
        sym(
            "avatar",
            &avatar_key,
            &[user],
            b"\x89PNG not really",
            encrypt_avatar(&avatar_key, user, b"\x89PNG not really"),
        ),
        {
            let name = "Chlo\u{e9} \u{304f}\u{308d}\u{3048}";
            let mut padded = name.as_bytes().to_vec();
            padded.resize(DISPLAY_NAME_PADDED, 0);
            let sealed = encrypt_display_name(&avatar_key, "chloe", name).unwrap();
            let mut v = sym("display-name", &avatar_key, &["chloe"], &padded, sealed);
            v["name"] = json!(name);
            v
        },
        {
            let d = PersonDetails {
                subject: "xe".into(),
                object: "xem".into(),
                possessive: "xyr".into(),
                gender: Some(Gender::Neuter),
            };
            let mut padded = serde_json::to_vec(&d).unwrap();
            padded.resize(PERSON_DETAILS_PADDED, 0);
            let sealed = encrypt_person_details(&avatar_key, "chloe", &d).unwrap();
            let mut v = sym("person-details", &avatar_key, &["chloe"], &padded, sealed);
            v["details"] = serde_json::to_value(&d).unwrap();
            v
        },
        sym(
            "node-key",
            &folder,
            &[node],
            node_key.as_bytes(),
            wrap_node_key(&folder, &node_key, node),
        ),
        {
            let mut v = sym("metadata", &node_key, &[node], &meta_padded, meta_sealed);
            v["metadata"] = serde_json::to_value(&meta).unwrap();
            v
        },
        {
            let mut v = sym(
                "metadata",
                &node_key,
                &[other_node],
                &old_meta_padded,
                old_meta_sealed,
            );
            v["metadata"] = serde_json::to_value(&old_meta).unwrap();
            v
        },
        sym(
            "content-key",
            &node_key,
            &[node, version],
            content_key.as_bytes(),
            wrap_content_key(&node_key, &content_key, node, version),
        ),
        sym(
            "link-key",
            &link_keys.kek,
            &[node],
            node_key.as_bytes(),
            wrap_link_key(&link_keys.kek, &node_key, node),
        ),
        sym(
            "link-secret",
            &node_key,
            &[node],
            link_secret.as_bytes(),
            encrypt_link_secret(&node_key, &link_secret, node),
        ),
        sym(
            "thumbnail",
            &node_key,
            &[node, version],
            b"\xff\xd8\xff\xe0 a small JPEG",
            encrypt_thumbnail(&node_key, node, version, b"\xff\xd8\xff\xe0 a small JPEG"),
        ),
        {
            let comment_id = "7c1d2e3f-4a5b-4c6d-9e7f-8a9b0c1d2e3f";
            let body = br#"{"text":"Looks good, one typo on page 2.","at":1790000123456}"#;
            sym(
                "comment",
                &node_key,
                &[node, comment_id, user],
                body,
                encrypt_comment(&node_key, node, comment_id, user, body),
            )
        },
        {
            let backup_key = key("backup key");
            let id = bytes("backup id", BACKUP_ID_LEN);
            let record = br#"E{"path":["Docs","a.txt"],"folder":false,"size":5,"mtime":0}"#;
            sym(
                "backup",
                &backup_key,
                &[&b64_encode(&id), "1"],
                record,
                seal_backup_record(&backup_key, &id, 1, record),
            )
        },
        sym(
            "chunk",
            &content_key,
            &[version, "0", "more"],
            &chunk0,
            encrypt_chunk(&content_key, version, 0, false, &chunk0),
        ),
        sym(
            "chunk",
            &content_key,
            &[version, "1", "last"],
            &chunk1,
            encrypt_chunk(&content_key, version, 1, true, &chunk1),
        ),
    ];
    let find =
        |list: &[Value], format: &str| list.iter().find(|v| v["format"] == format).unwrap().clone();
    let rejects = vec![
        reject(
            &find(&symmetric, "node-key"),
            json!({ "context": [other_node] }),
            "a node key moved to another node",
        ),
        reject(
            &find(&symmetric, "metadata"),
            json!({ "context": [other_node] }),
            "metadata moved to another node",
        ),
        reject(
            &find(&symmetric, "content-key"),
            json!({ "context": [node, other_node] }),
            "a content key moved to another version",
        ),
        reject(
            &find(&symmetric, "chunk"),
            json!({ "context": [version, "0", "last"] }),
            "a file cut short after its first chunk",
        ),
        reject(
            &find(&symmetric, "chunk"),
            json!({ "context": [version, "1", "more"] }),
            "chunks swapped",
        ),
        reject(
            &find(&symmetric, "link-key"),
            json!({ "key": b64(derive_link_password_keys(link_secret.as_bytes(), "open says me").unwrap().kek.as_bytes()) }),
            "a link's key with the wrong password",
        ),
        reject(
            &find(&symmetric, "thumbnail"),
            json!({ "context": [node, other_node] }),
            "an old version's thumbnail shown for a new one",
        ),
        {
            let v = find(&symmetric, "comment");
            let ctx = v["context"].clone();
            reject(
                &v,
                json!({ "context": [ctx[0], ctx[1], other_node] }),
                "a comment put in someone else's name",
            )
        },
        {
            let v = find(&symmetric, "backup");
            let id = v["context"][0].clone();
            reject(
                &v,
                json!({ "context": [id, "2"] }),
                "a backup's records reordered",
            )
        },
        reject(
            &find(&symmetric, "private-data"),
            json!({ "context": [user, "music"] }),
            "private data read under another label",
        ),
        reject(
            &find(&symmetric, "master-key-app"),
            json!({ "context": [other_node] }),
            "an app password's master key copy used for another app password",
        ),
        reject(
            &find(&symmetric, "master-key"),
            json!({ "key": b64(mk.as_bytes()) }),
            "the wrong key",
        ),
        reject(
            &find(&symmetric, "person-details"),
            json!({ "context": ["alice"] }),
            "someone's pronouns shown as another person's",
        ),
        reject(
            &find(&symmetric, "display-name"),
            json!({ "context": ["alice"] }),
            "someone's display name shown as another person's",
        ),
        {
            // Sealed properly, but not a name a client may show.
            let mut pt = "alice\u{202e}gnp.exe".as_bytes().to_vec();
            pt.resize(DISPLAY_NAME_PADDED, 0);
            let sealed = seal(&avatar_key, &pt, &aad("display-name", &["chloe"]));
            let v = sym("display-name", &avatar_key, &["chloe"], &pt, sealed);
            reject(
                &v,
                json!({}),
                "a display name with a right-to-left override",
            )
        },
        {
            let mut v = find(&symmetric, "avatar");
            let mut s = raw(&v, "sealed");
            let last = s.len() - 1;
            s[last] ^= 1;
            v["sealed"] = b64(&s);
            reject(&v, json!({}), "one bit of the tag flipped")
        },
        {
            let mut v = find(&symmetric, "node-key");
            let mut s = raw(&v, "sealed");
            s[0] = FORMAT_VERSION + 1;
            v["sealed"] = b64(&s);
            reject(&v, json!({}), "a format version this client doesn't know")
        },
        {
            let mut v = find(&symmetric, "node-key");
            let s = raw(&v, "sealed");
            v["sealed"] = b64(&s[1..]);
            reject(&v, json!({}), "the version byte left out")
        },
    ];
    symmetric.extend(rejects);

    // Sealed boxes: to the classic key and to the hybrid one.
    let mut sealed_boxes = vec![];
    for (kp, kind) in [(&classic, "x25519"), (&hybrid, "x25519+ml-kem-768")] {
        let secret = [
            kp.secret.as_bytes().as_slice(),
            kp.pq.as_ref().map_or(&[][..], |p| p.seed().as_slice()),
        ]
        .concat();
        let to = kp.sealing_key();
        let entry = |format: &str, ctx: &[&str], sealed: Vec<u8>, pt: &Key| {
            json!({
                "format": format,
                "recipient": kind,
                "secret": b64(&secret),
                "sealing_key": b64(&to),
                "context": ctx,
                "aad": b64(&aad(format, ctx)),
                "plaintext": b64(pt.as_bytes()),
                "sealed": b64(&sealed),
            })
        };
        sealed_boxes.push(entry(
            "share",
            &[node],
            seal_share_key(&to, &node_key, node).unwrap(),
            &node_key,
        ));
        sealed_boxes.push(entry(
            "drop",
            &[node, folder_id],
            seal_drop_key(&to, &node_key, node, folder_id).unwrap(),
            &node_key,
        ));
        sealed_boxes.push(entry(
            "avatar-key",
            &[user, other_node],
            seal_avatar_key(&to, &avatar_key, user, other_node).unwrap(),
            &avatar_key,
        ));
    }
    let hybrid_share = sealed_boxes[3].clone();
    let classic_share = sealed_boxes[0].clone();
    sealed_boxes.extend([
        reject(
            &classic_share,
            json!({ "context": [other_node] }),
            "a share key presented as another node's",
        ),
        reject(
            &hybrid_share,
            json!({ "secret": b64(hybrid.secret.as_bytes()) }),
            "a hybrid box opened without the ML-KEM key",
        ),
        reject(
            &classic_share,
            json!({ "secret": b64(hybrid.secret.as_bytes()) }),
            "a box opened by someone else",
        ),
        {
            let mut s = raw(&classic_share, "sealed");
            s[0] = 3;
            reject(
                &classic_share,
                json!({ "sealed": b64(&s) }),
                "a sealed box of a kind this client doesn't know",
            )
        },
    ]);

    let file = json!({
        "about": "Test vectors for docs/format/README.md. Bytes are base64url without padding; strings are UTF-8. Every ciphertext must open to its plaintext under its key and context, except those marked \"valid\": false, which must not open. Made by crates/thencloud-crypto/tests/vectors.rs.",
        "account_keys": account_keys,
        "recovery_keys": recovery_keys,
        "app_password_keys": app_password_keys,
        "passkeys": passkeys,
        "link_passwords": link_passwords,
        "keypairs": keypairs,
        "name_tags": name_tags,
        "padding": padding,
        "symmetric": symmetric,
        "sealed_boxes": sealed_boxes,
    });
    let mut out = serde_json::to_string_pretty(&merge(file)).unwrap();
    out.push('\n');
    std::fs::create_dir_all(std::path::Path::new(PATH).parent().unwrap()).unwrap();
    std::fs::write(PATH, out).unwrap();
}

/// Add what's new in `file` to the vectors already written, keeping every
/// existing entry as it is. An entry counts as already there when one has the
/// same fields apart from `sealed` (ciphertexts have random nonces, so a new
/// run never repeats one). So if a change makes a key or tag come out
/// differently, the new value is added next to the old one, and
/// `vectors_match` fails on the old one instead of the change going unseen.
fn merge(mut file: Value) -> Value {
    let Ok(text) = std::fs::read_to_string(PATH) else {
        return file;
    };
    let old: Value = serde_json::from_str(&text).expect("docs/format/vectors.json isn't JSON");
    let same = |a: &Value, b: &Value| {
        let strip = |v: &Value| {
            let mut v = v.clone();
            if let Some(o) = v.as_object_mut() {
                o.remove("sealed");
            }
            v
        };
        strip(a) == strip(b)
    };
    let Some(sections) = old.as_object() else {
        return file;
    };
    for (name, kept) in sections {
        let Some(kept) = kept.as_array() else {
            continue;
        };
        let fresh = file[name].as_array().cloned().unwrap_or_default();
        let mut all = kept.clone();
        for entry in fresh {
            if !all.iter().any(|k| same(k, &entry)) {
                all.push(entry);
            }
        }
        file[name] = Value::Array(all);
    }
    file
}

fn open_raw(k: &Key, sealed: &[u8], aad: &[u8]) -> Vec<u8> {
    open(k, sealed, aad).unwrap()
}
