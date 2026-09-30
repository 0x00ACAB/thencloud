# File reports Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** People who can open a file they don't own can report one version of it to the admins, who can open that version in their own browser and remove the file or mark it safe.

**Architecture:** The reporter's browser seals a padded JSON record (the version's content key, name, MIME type, size, note) to every admin's public key with a new `report` sealed box, bound to node id and version id. The server stores reports and sealed boxes, never plaintext, and keeps the reported version's blobs until the report is closed, even if the owner deletes the file meanwhile. Admins list reports, open their box, fetch the version's chunks through an admin route and decrypt in the browser.

**Tech Stack:** Rust (thencloud-crypto, axum + sqlx SQLite in thencloud-server, wasm-bindgen), Svelte 5 + Vite web client, Playwright.

**Spec:** `docs/superpowers/specs/2026-09-30-file-reports-design.md`

## Global Constraints

- No key, password or plaintext reaches the server: the report record is sealed in the browser; the server stores only sealed boxes, ids, a reason code and times.
- New ciphertext formats bind their context as AEAD associated data (`aad("report", &[node_id, version_id])`), are described in `docs/format/README.md` and have vectors (`crates/thencloud-crypto/tests/vectors.rs` writes `docs/format/vectors.json`; checked by `web/tests/vectors.test.js`). Unreleased: the vectors file may be regenerated.
- Every new server feature is in the zero-knowledge scan in `crates/thencloud-server/tests/e2e.rs`; new browser flows go in `web/e2e/` with `watchRequests`.
- Reasons are exactly `illegal`, `malware`, `copyright`, `harassment`, `other`.
- Note at most 2,000 characters; name at most 1,000 characters (the browser shortens longer names); record padded to exactly 16,384 bytes.
- Removing is silent: the owner is not told. Remove and mark-safe go through `audit::record`.
- Web text goes through `t()` with Polish and German translations (`web/src/lib/messages/{pl,de}.js`), or `npm test` fails. No emoji, no `—` or `…` in copy, colours from semantic tokens only, no inline `style=`.
- Keep `cargo clippy --workspace --all-targets`, `cargo fmt --all --check` and `cd web && npm run check` at zero warnings.
- Commit messages end with `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`. Work on branch `file-reports`; never merge or push without asking.

## Review Focus

1. The owner deletes (trash, empty trash, delete an old version, version thinning, quota pruning) a file while a report on it is open: the admin must still be able to open the reported version. Pinned in Task 4.
2. A report whose admin boxes don't cover every current admin, or whose boxes are the wrong size: refused, so no admin is silently left out and nothing extra can be stuffed in. Pinned in Task 3.
3. A second admin made after the report: can't open it until another admin reseals; the reseal route must only accept boxes for real admins who lack one. Pinned in Task 4.
4. A link visitor on a password or `max_opens` link reports without the link token: refused like any other link route. Pinned in Task 3.
5. A link visitor floods reports: 11th within 15 minutes from one address refused; 21st open report on one node refused. Pinned in Task 3.

---

### Task 0: Fix link download status on the share page

`web/src/SharePage.svelte` sets `t.status`/`t.error` (the translation function) instead of the job's, so link downloads never show as done or failed.

**Files:**
- Modify: `web/src/SharePage.svelte` (functions `downloadAll`, `downloadEntry`)

- [ ] **Step 1: Fix the assignments**

In `downloadAll`, replace the catch block's `t.status = 'error'; t.error = errorMessage(e);` with `job.status = 'error'; job.error = errorMessage(e);`. In `downloadEntry`, replace `t.status = 'done';` with `job.status = 'done';` and the catch block's two `t.` lines with `job.status = 'error'; job.error = errorMessage(e);`.

- [ ] **Step 2: Check**

Run: `cd web && npm run check && npm test`
Expected: 0 errors, 0 warnings; all tests pass.

- [ ] **Step 3: Commit**

```bash
git add web/src/SharePage.svelte
git commit -m "Mark link downloads done or failed in the transfer tray

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 1: The `report` sealed box (crypto, WASM, vectors, format docs)

**Files:**
- Modify: `crates/thencloud-crypto/src/lib.rs` (after `open_drop_key`)
- Modify: `crates/thencloud-crypto/tests/vectors.rs` (`open_box`, the sealed-box entries loop)
- Modify: `crates/thencloud-wasm/src/lib.rs` (after `open_drop_key`)
- Modify: `web/tests/vectors.test.js` (`openBox`)
- Modify: `docs/format/README.md` (sealed box table near line 316)
- Regenerate: `docs/format/vectors.json`

**Interfaces:**
- Produces (Rust, `thencloud_crypto`):
  - `pub struct ReportRecord { pub content_key: String /* base64url of 32 bytes */, pub name: String, pub mime: Option<String>, pub size: u64, pub note: String }` (serde, `Clone`, `Debug`, `PartialEq`)
  - `pub const REPORT_PADDED: usize = 16384;`, `pub const MAX_REPORT_NOTE_CHARS: usize = 2000;`, `pub const MAX_REPORT_NAME_CHARS: usize = 1000;`
  - `pub const REPORT_SEALED_LEN: usize` (X25519 box) and `pub const REPORT_SEALED_HYBRID_LEN: usize` (hybrid box)
  - `pub fn seal_report(admin_pub: &[u8], record: &ReportRecord, node_id: &str, version_id: &str) -> Result<Vec<u8>>`
  - `pub fn open_report(kp: &KeyPair, sealed: &[u8], node_id: &str, version_id: &str) -> Result<ReportRecord>`
- Produces (WASM): `seal_report(admin_public, record_json, node_id, version_id) -> Uint8Array`, `open_report(secret, sealed, node_id, version_id) -> string` (the record as JSON).

- [ ] **Step 1: Write failing unit tests** in the `#[cfg(test)] mod tests` of `crates/thencloud-crypto/src/lib.rs`:

```rust
#[test]
fn report_boxes_open_only_for_their_version() {
    let admin = KeyPair::generate().with_pq(PqKeyPair::generate());
    let rec = ReportRecord {
        content_key: b64_encode(Key::generate().as_bytes()),
        name: "evil.exe".into(),
        mime: Some("application/octet-stream".into()),
        size: 1234,
        note: "phishing".into(),
    };
    let sealed = seal_report(&admin.sealing_key(), &rec, "node", "v1").unwrap();
    assert_eq!(sealed.len(), REPORT_SEALED_HYBRID_LEN);
    assert_eq!(open_report(&admin, &sealed, "node", "v1").unwrap(), rec);
    assert!(open_report(&admin, &sealed, "node", "v2").is_err());
    assert!(open_report(&admin, &sealed, "other", "v1").is_err());
    let classic = KeyPair::generate();
    let s2 = seal_report(&classic.public, &rec, "node", "v1").unwrap();
    assert_eq!(s2.len(), REPORT_SEALED_LEN);
}

#[test]
fn report_records_are_checked() {
    let admin = KeyPair::generate();
    let ok = ReportRecord {
        content_key: b64_encode(Key::generate().as_bytes()),
        name: "a".into(),
        mime: None,
        size: 1,
        note: String::new(),
    };
    for bad in [
        ReportRecord { note: "x".repeat(MAX_REPORT_NOTE_CHARS + 1), ..ok.clone() },
        ReportRecord { name: "x".repeat(MAX_REPORT_NAME_CHARS + 1), ..ok.clone() },
        ReportRecord { content_key: b64_encode(&[1; 16]), ..ok.clone() },
    ] {
        assert!(seal_report(&admin.public, &bad, "n", "v").is_err());
    }
    // Worst case still fits the padding: four-byte characters everywhere.
    let big = ReportRecord {
        name: "\u{1F600}".repeat(MAX_REPORT_NAME_CHARS),
        note: "\u{1F600}".repeat(MAX_REPORT_NOTE_CHARS),
        mime: Some("x".repeat(255)),
        ..ok
    };
    assert!(seal_report(&admin.public, &big, "n", "v").is_ok());
}
```

(Check the existing test module for how a `KeyPair`/`PqKeyPair` is generated in tests and use the same constructors; adjust names if they differ, e.g. `KeyPair::generate()`.)

- [ ] **Step 2: Run to see them fail**

Run: `cargo test -p thencloud-crypto report_`
Expected: compile errors, `seal_report` not found.

- [ ] **Step 3: Implement** after `open_drop_key`:

```rust
// ---------------------------------------------------------------------------
// File reports: someone who can open a file they don't own shows one version
// of it to the server's admins. The record is sealed to each admin.
// ---------------------------------------------------------------------------

/// A report record is padded to this many bytes before sealing, so every
/// report box is the same size whatever the name and note.
pub const REPORT_PADDED: usize = 16384;
pub const MAX_REPORT_NOTE_CHARS: usize = 2000;
pub const MAX_REPORT_NAME_CHARS: usize = 1000;
/// A report sealed to an X25519 key alone, and to an X25519 + ML-KEM key.
pub const REPORT_SEALED_LEN: usize = 1 + 32 + SEALED_OVERHEAD + REPORT_PADDED;
pub const REPORT_SEALED_HYBRID_LEN: usize = REPORT_SEALED_LEN + PQ_CIPHERTEXT_LEN;

/// What a reporter shows the admins: the reported version's content key
/// (base64url), the file's name, type and size, and a note.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReportRecord {
    pub content_key: String,
    pub name: String,
    #[serde(default)]
    pub mime: Option<String>,
    pub size: u64,
    #[serde(default)]
    pub note: String,
}

impl ReportRecord {
    fn check(&self) -> Result<()> {
        let key = b64_decode(&self.content_key)?;
        if key.len() != KEY_LEN
            || self.name.chars().count() > MAX_REPORT_NAME_CHARS
            || self.note.chars().count() > MAX_REPORT_NOTE_CHARS
            || self.mime.as_ref().is_some_and(|m| m.len() > 255)
        {
            return Err(Error::Metadata("not a valid report".into()));
        }
        Ok(())
    }
}

pub fn seal_report(
    admin_pub: &[u8],
    record: &ReportRecord,
    node_id: &str,
    version_id: &str,
) -> Result<Vec<u8>> {
    record.check()?;
    let mut pt = serde_json::to_vec(record).map_err(|e| Error::Metadata(e.to_string()))?;
    if pt.len() > REPORT_PADDED {
        return Err(Error::Metadata("report too long".into()));
    }
    pt.resize(REPORT_PADDED, 0);
    let sealed = seal_to_public(admin_pub, &pt, &aad("report", &[node_id, version_id]));
    pt.zeroize();
    sealed
}

/// Opens a report, refusing any that `seal_report` wouldn't have written.
pub fn open_report(
    kp: &KeyPair,
    sealed: &[u8],
    node_id: &str,
    version_id: &str,
) -> Result<ReportRecord> {
    let mut pt = open_sealed(kp, sealed, &aad("report", &[node_id, version_id]))?;
    let end = pt.iter().position(|&b| b == 0).unwrap_or(pt.len());
    let parsed = if pt.len() != REPORT_PADDED || pt[end..].iter().any(|&b| b != 0) {
        Err(Error::Metadata("bad report padding".into()))
    } else {
        serde_json::from_slice::<ReportRecord>(&pt[..end])
            .map_err(|e| Error::Metadata(e.to_string()))
    };
    pt.zeroize();
    let record = parsed?;
    record.check()?;
    Ok(record)
}
```

Worst case check: 1000 × 4 + 2000 × 4 + 255 + 44 + ~80 bytes of JSON keys and numbers ≈ 12.4 KiB < 16 KiB. JSON escapes of control characters could grow a name up to 6×; if the worst-case test fails for that reason, reject names/notes whose JSON exceeds `REPORT_PADDED` (already done by the length check) and keep the test on four-byte characters only.

- [ ] **Step 4: Run the unit tests**

Run: `cargo test -p thencloud-crypto report_`
Expected: PASS.

- [ ] **Step 5: WASM wrappers** in `crates/thencloud-wasm/src/lib.rs` after `open_drop_key`:

```rust
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
```

(Match how `encrypt_person_details` in the same file turns a serde error into `R`'s error; copy that exact conversion.)

- [ ] **Step 6: Vectors.** In `tests/vectors.rs`:
  - In the sealed-boxes loop after the `avatar-key` entry, push a `report` entry. The `entry` closure takes a `&Key` plaintext; add a sibling entry for reports that records the padded plaintext bytes instead:

```rust
let record = ReportRecord {
    content_key: b64(content_key.as_bytes()),
    name: "report.pdf".into(),
    mime: Some("application/pdf".into()),
    size: 4096,
    note: "a note for the admins".into(),
};
let mut padded = serde_json::to_vec(&record).unwrap();
padded.resize(REPORT_PADDED, 0);
let mut e = entry(
    "report",
    &[node, version_id],
    seal_report(&to, &record, node, version_id).unwrap(),
    &avatar_key, // replaced below
);
e["plaintext"] = json!(b64(&padded));
sealed_boxes.push(e);
```

  Use whatever content key and version id variables the file already defines for the `content-key` vector (search for `wrap_content_key(`); if the names differ, use those.
  - In `open_box`, `open_box` returns key bytes; add a branch that returns the padded record instead:

```rust
"report" => {
    let r = open_report(&kp, &sealed, &ctx[0], &ctx[1])?;
    let mut pt = serde_json::to_vec(&r).unwrap();
    pt.resize(REPORT_PADDED, 0);
    return Ok(pt);
}
```

  - Add one rejection: the report box presented for another version (`json!({ "context": [node, other_node] })`, why `"a report opened as another version's"`).
  - In `web/tests/vectors.test.js` `openBox`, add:

```js
case 'report': {
  const r = new TextEncoder().encode(tc.open_report(s, sealed, c[0], c[1]));
  const out = new Uint8Array(v.plaintext ? bytes(v.plaintext).length : r.length);
  out.set(r);
  return out;
}
```

- [ ] **Step 7: Format docs.** In `docs/format/README.md`, add a row to the sealed-box table:

```
| `report` | a file report: JSON `{content_key, name, mime, size, note}` padded with zeros to 16,384 bytes, sealed to each admin | `aad("report", node_id, version_id)` |
```

and change the sentence "They're used for shares, file drops and avatar keys." to "They're used for shares, file drops, avatar keys and file reports."

- [ ] **Step 8: Regenerate and run everything**

Run: `cargo test -p thencloud-crypto && ./build.sh && (cd web && npm test)`
Expected: vectors.json rewritten (see how `tests/vectors.rs` writes it; run with the env var or flag it documents), all Rust and JS vector tests pass.

- [ ] **Step 9: Commit**

```bash
git add crates/thencloud-crypto crates/thencloud-wasm web/tests/vectors.test.js docs/format
git commit -m "Seal file reports to admins

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 2: Reports table, holding reported versions, and reporting as a signed-in user

**Files:**
- Create: `crates/thencloud-server/migrations/0026_reports.sql`
- Create: `crates/thencloud-server/src/routes/reports.rs`
- Modify: `crates/thencloud-server/src/routes/mod.rs` (module + routes)
- Modify: `crates/thencloud-server/src/error.rs` (`TooManyReports`)
- Modify: `crates/thencloud-crypto/src/api.rs` (wire types)
- Test: `crates/thencloud-server/tests/e2e.rs` (new test `reports_by_share_recipients`)

**Interfaces:**
- Produces (api.rs):

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReportReason { Illegal, Malware, Copyright, Harassment, Other }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReportKey { pub admin_id: String, pub public_key: B64, #[serde(default)] pub pq_public_key: Option<B64> }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReportBox { pub admin_id: String, pub sealed: B64 }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateReportRequest {
    pub id: String,
    pub node_id: String,
    pub version_id: String,
    pub reason: ReportReason,
    pub boxes: Vec<ReportBox>,
}
```

- Produces (server): `reports::create(state: &AppState, reporter: Reporter<'_>, node: &NodeRow, req: CreateReportRequest) -> Result<StatusCode>` with `pub enum Reporter<'a> { User(&'a str), Link { ip: Option<String> } }`; `reports::delete_versions(state: &AppState, ids: &[String])`; `reports::held(db: &SqlitePool, ids: &[String]) -> Result<HashSet<String>>`.
- Routes: `GET /api/report-keys` → `Vec<ReportKey>`; `POST /api/reports` → 204.

- [ ] **Step 1: Migration** `0026_reports.sql`:

```sql
-- File reports (#108). Someone who can open a file they don't own shows one
-- version of it to the admins: the version's content key, the name, type and
-- a note are sealed to each admin in the reporter's browser, so the server
-- keeps only sealed boxes. While a report is open, the reported version's
-- blobs are kept even if the file is deleted (no foreign key to nodes).
CREATE TABLE reports (
    id          TEXT PRIMARY KEY,
    node_id     TEXT NOT NULL,
    version_id  TEXT NOT NULL,
    chunk_count INTEGER NOT NULL,
    owner_id    TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    reason      TEXT NOT NULL
                CHECK (reason IN ('illegal', 'malware', 'copyright', 'harassment', 'other')),
    reporter_id TEXT REFERENCES users(id) ON DELETE SET NULL,
    via_link    INTEGER NOT NULL DEFAULT 0,
    created_at  INTEGER NOT NULL,
    status      TEXT NOT NULL DEFAULT 'open' CHECK (status IN ('open', 'safe', 'removed')),
    handled_by  TEXT,
    handled_at  INTEGER
);
CREATE INDEX reports_status ON reports(status, created_at);
CREATE INDEX reports_open_versions ON reports(version_id) WHERE status = 'open';
CREATE INDEX reports_node ON reports(node_id);
CREATE UNIQUE INDEX reports_one_per_reporter ON reports(node_id, reporter_id)
    WHERE status = 'open' AND reporter_id IS NOT NULL;

CREATE TABLE report_boxes (
    report_id TEXT NOT NULL REFERENCES reports(id) ON DELETE CASCADE,
    admin_id  TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    sealed    BLOB NOT NULL,
    PRIMARY KEY (report_id, admin_id)
);
```

- [ ] **Step 2: Error.** In `error.rs` add `#[error("too many reports; try again later")] TooManyReports,` mapped to `(StatusCode::TOO_MANY_REQUESTS, "too_many_reports")`.

- [ ] **Step 3: Write the failing test** in `tests/e2e.rs` (put helpers above it):

```rust
/// Seal a report of `node`'s current version to every admin the server lists.
async fn report_boxes(
    h: &Harness,
    keys_uri: &str,
    token: Option<&str>,
    headers: &[(&str, &str)],
    node: &Node,
    node_key: &Key,
    note: &str,
) -> Vec<ReportBox> {
    let r = h.raw(Method::GET, keys_uri, token, headers, Body::empty(), None).await;
    assert_eq!(r.status, StatusCode::OK, "{r:?}");
    let keys: Vec<ReportKey> = r.json();
    let v = node.version.as_ref().unwrap();
    let ck = c::unwrap_content_key(node_key, &v.enc_content_key, &node.id, &v.id).unwrap();
    let m = c::decrypt_metadata(node_key, &node.id, &node.enc_metadata).unwrap();
    let rec = c::ReportRecord {
        content_key: c::b64_encode(ck.as_bytes()),
        name: m.name,
        mime: m.mime,
        size: m.size,
        note: note.into(),
    };
    keys.iter()
        .map(|k| {
            let mut to = k.public_key.0.clone();
            if let Some(pq) = &k.pq_public_key {
                to.extend_from_slice(pq);
            }
            ReportBox {
                admin_id: k.admin_id.clone(),
                sealed: B64(c::seal_report(&to, &rec, &node.id, &v.id).unwrap()),
            }
        })
        .collect()
}

fn report_req(node: &Node, reason: ReportReason, boxes: Vec<ReportBox>) -> CreateReportRequest {
    CreateReportRequest {
        id: c::new_id(),
        node_id: node.id.clone(),
        version_id: node.version.as_ref().unwrap().id.clone(),
        reason,
        boxes,
    }
}

#[tokio::test]
async fn reports_by_share_recipients() {
    let h = Harness::new().await;
    let admin = register(&h, "root", "admin pw").await;
    let alice = register(&h, "alice", "alice pw").await;
    let bob = register(&h, "bob", "bob pw").await;
    let carol = register(&h, "carol", "carol pw").await;
    let (folder, folder_key) = alice.mkdir(&h, &alice.root, "Shared").await;
    let file = alice
        .upload(&h, &folder, None, "invoice.pdf.exe", &secret_payload(5000))
        .await
        .unwrap();
    let file_key = c::unwrap_node_key(&folder_key, &file.enc_key, &file.id).unwrap();
    let pk: UserPublicKey = h.get("/api/users/bob/public-key", &alice.token).await.json();
    let r = h
        .call(Method::POST, "/api/shares", Some(&alice.token), Some(CreateShareRequest {
            node_id: folder.clone(),
            recipient: "bob".into(),
            wrapped_key: B64(c::seal_share_key(&sealing_key(&pk), &folder_key, &folder).unwrap()),
            permission: Permission::Read,
            expires_at: None,
        }))
        .await;
    assert_eq!(r.status, StatusCode::CREATED);

    let boxes = report_boxes(&h, "/api/report-keys", Some(&bob.token), &[], &file, &file_key, "PHISHING-NOTE-MARKER").await;
    assert_eq!(boxes.len(), 1, "one admin");
    let post = |token: &str, req: &CreateReportRequest| {
        let (token, req) = (token.to_string(), req.clone());
        let h = &h;
        async move { h.call(Method::POST, "/api/reports", Some(&token), Some(req)).await }
    };

    // Owners can't report their own files, strangers can't see them.
    let req = report_req(&file, ReportReason::Malware, boxes.clone());
    assert_eq!(post(&alice.token, &req).await.status, StatusCode::FORBIDDEN);
    assert_eq!(post(&carol.token, &req).await.status, StatusCode::NOT_FOUND);
    // Every admin needs a box, and boxes must be report-sized.
    let none = report_req(&file, ReportReason::Malware, vec![]);
    assert_eq!(post(&bob.token, &none).await.status, StatusCode::BAD_REQUEST);
    let mut short = boxes.clone();
    short[0].sealed.0.truncate(100);
    let short = report_req(&file, ReportReason::Malware, short);
    assert_eq!(post(&bob.token, &short).await.status, StatusCode::BAD_REQUEST);
    // A version that isn't the node's is refused.
    let mut wrong = req.clone();
    wrong.version_id = c::new_id();
    assert_eq!(post(&bob.token, &wrong).await.status, StatusCode::BAD_REQUEST);

    assert_eq!(post(&bob.token, &req).await.status, StatusCode::NO_CONTENT);
    // One open report per person per file.
    let again = report_req(&file, ReportReason::Other, boxes.clone());
    assert_eq!(post(&bob.token, &again).await.status, StatusCode::CONFLICT);

    // The admin can open what bob sealed.
    let sealed: Vec<u8> = sqlx::query_scalar("SELECT sealed FROM report_boxes")
        .fetch_one(&h.state.db)
        .await
        .unwrap();
    let rec = c::open_report(&admin.kp, &sealed, &file.id, &file.version.as_ref().unwrap().id).unwrap();
    assert_eq!(rec.name, "invoice.pdf.exe");
    assert_eq!(rec.note, "PHISHING-NOTE-MARKER");

    // Zero knowledge: no name, note or content anywhere on disk.
    let mut files = Vec::new();
    all_files(h.dir.path(), &mut files);
    for f in files {
        let data = std::fs::read(&f).unwrap();
        for needle in [&b"invoice.pdf.exe"[..], b"PHISHING-NOTE-MARKER", MARKER] {
            assert!(!contains(&data, needle), "{} holds {:?}", f.display(), String::from_utf8_lossy(needle));
        }
    }
}
```

(`CreateReportRequest` must derive `Clone`. If the database may still be in the WAL when scanned, do what `full_lifecycle_is_zero_knowledge` does before its scan, e.g. a checkpoint, copying that exact code.)

- [ ] **Step 4: Run to see it fail**

Run: `cargo test -p thencloud-server --test e2e reports_by_share_recipients`
Expected: compile error (types missing).

- [ ] **Step 5: Implement `routes/reports.rs`** (first part; Task 4 adds the admin routes):

```rust
//! File reports (#108). Someone who can open a file they don't own shows one
//! version of it to the admins: its content key, name, type and a note, sealed
//! to each admin in their browser (`seal_report`). The server never sees any
//! of it. While a report is open the reported version's blobs are kept, even
//! if the owner deletes the file, until an admin removes it or marks it safe.

use std::collections::HashSet;

use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use sqlx::SqlitePool;
use thencloud_crypto::api::*;
use thencloud_crypto::{REPORT_SEALED_HYBRID_LEN, REPORT_SEALED_LEN};

use crate::AppState;
use crate::access::{self, Access};
use crate::auth::AuthUser;
use crate::db::{NodeRow, get_node};
use crate::error::{AppError, Result};
use crate::util::{check_id, coarse_now};

/// Open reports on one node before more are refused (link visitors).
const MAX_OPEN_PER_NODE: i64 = 20;

pub enum Reporter<'a> {
    User(&'a str),
    /// A public link's visitor, rate limited by address (never stored).
    Link { ip: Option<String> },
}

fn reason_str(r: ReportReason) -> &'static str {
    match r {
        ReportReason::Illegal => "illegal",
        ReportReason::Malware => "malware",
        ReportReason::Copyright => "copyright",
        ReportReason::Harassment => "harassment",
        ReportReason::Other => "other",
    }
}

/// Every admin's sealing key, for `seal_report`. Ids, not usernames.
pub async fn admin_keys(db: &SqlitePool) -> Result<Vec<ReportKey>> {
    let rows: Vec<(String, Vec<u8>, Option<Vec<u8>>)> = sqlx::query_as(
        "SELECT id, public_key, pq_public_key FROM users \
         WHERE is_admin = 1 AND disabled_at IS NULL ORDER BY id",
    )
    .fetch_all(db)
    .await?;
    Ok(rows
        .into_iter()
        .map(|(admin_id, pk, pq)| ReportKey {
            admin_id,
            public_key: B64(pk),
            pq_public_key: pq.map(B64),
        })
        .collect())
}

pub async fn keys(State(state): State<AppState>, _user: AuthUser) -> Result<Json<Vec<ReportKey>>> {
    Ok(Json(admin_keys(&state.db).await?))
}

/// Boxes must be report-sized and cover exactly the admins in `admins`.
fn check_boxes(boxes: &[ReportBox], admins: &[ReportKey]) -> Result<()> {
    let want: HashSet<&str> = admins.iter().map(|a| a.admin_id.as_str()).collect();
    let got: HashSet<&str> = boxes.iter().map(|b| b.admin_id.as_str()).collect();
    if want.is_empty() || got != want || got.len() != boxes.len() {
        return Err(AppError::bad("a report needs one box for each admin"));
    }
    for b in boxes {
        if b.sealed.len() != REPORT_SEALED_LEN && b.sealed.len() != REPORT_SEALED_HYBRID_LEN {
            return Err(AppError::bad("sealed has an invalid size"));
        }
    }
    Ok(())
}

pub async fn create_route(
    State(state): State<AppState>,
    user: AuthUser,
    Json(req): Json<CreateReportRequest>,
) -> Result<StatusCode> {
    check_id(&req.node_id, "node_id")?;
    // Not visible, trashed or not there: all look the same.
    if access::access(&state.db, &user.id, &req.node_id).await?.is_none() {
        return Err(AppError::NotFound);
    }
    let node = get_node(&state.db, &req.node_id).await?.ok_or(AppError::NotFound)?;
    if node.owner_id == user.id {
        return Err(AppError::Forbidden);
    }
    create(&state, Reporter::User(&user.id), &node, req).await
}

/// Store a report on `node` (already checked to be visible to the reporter).
pub async fn create(
    state: &AppState,
    reporter: Reporter<'_>,
    node: &NodeRow,
    req: CreateReportRequest,
) -> Result<StatusCode> {
    check_id(&req.id, "id")?;
    check_id(&req.version_id, "version_id")?;
    if node.is_folder() {
        return Err(AppError::bad("only files can be reported"));
    }
    let chunks: Option<i64> =
        sqlx::query_scalar("SELECT chunk_count FROM file_versions WHERE id = ? AND node_id = ?")
            .bind(&req.version_id)
            .bind(&node.id)
            .fetch_optional(&state.db)
            .await?;
    let chunks = chunks.ok_or_else(|| AppError::bad("not a version of this file"))?;
    check_boxes(&req.boxes, &admin_keys(&state.db).await?)?;

    let (reporter_id, via_link) = match &reporter {
        Reporter::User(id) => (Some(*id), false),
        Reporter::Link { ip } => {
            let key = format!("report:{}", ip.clone().unwrap_or_default());
            if state.limiter.blocked(&key) {
                return Err(AppError::TooManyReports);
            }
            state.limiter.fail(&key);
            (None, true)
        }
    };
    let open: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM reports WHERE node_id = ? AND status = 'open'")
            .bind(&node.id)
            .fetch_one(&state.db)
            .await?;
    if open >= MAX_OPEN_PER_NODE {
        return Err(AppError::TooManyReports);
    }

    let mut tx = state.db.begin().await?;
    let r = sqlx::query(
        "INSERT INTO reports (id, node_id, version_id, chunk_count, owner_id, reason, reporter_id, via_link, created_at) \
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&req.id)
    .bind(&node.id)
    .bind(&req.version_id)
    .bind(chunks)
    .bind(&node.owner_id)
    .bind(reason_str(req.reason))
    .bind(reporter_id)
    .bind(via_link)
    .bind(coarse_now())
    .execute(&mut *tx)
    .await;
    match r {
        Err(sqlx::Error::Database(d)) if d.is_unique_violation() => {
            return Err(AppError::Conflict("you've already reported this file".into()));
        }
        r => r?,
    };
    for b in &req.boxes {
        sqlx::query("INSERT INTO report_boxes (report_id, admin_id, sealed) VALUES (?, ?, ?)")
            .bind(&req.id)
            .bind(&b.admin_id)
            .bind(&b.sealed.0)
            .execute(&mut *tx)
            .await?;
    }
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}

/// Of `ids`, the versions an open report is holding.
pub async fn held(db: &SqlitePool, ids: &[String]) -> Result<HashSet<String>> {
    let mut out = HashSet::new();
    for id in ids {
        let n: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM reports WHERE version_id = ? AND status = 'open'",
        )
        .bind(id)
        .fetch_one(db)
        .await?;
        if n > 0 {
            out.insert(id.clone());
        }
    }
    Ok(out)
}

/// Delete versions' blobs, except those an open report is holding: they go
/// when the report is closed (`release`).
pub async fn delete_versions(state: &AppState, ids: &[String]) {
    let keep = match held(&state.db, ids).await {
        Ok(k) => k,
        // Keep everything rather than lose what a report needs; `check`
        // lists what's left over.
        Err(e) => {
            tracing::warn!(error = %e, "couldn't check reports before deleting blobs");
            return;
        }
    };
    let gone: Vec<String> = ids.iter().filter(|i| !keep.contains(*i)).cloned().collect();
    state.blobs.delete_versions(&gone).await;
}
```

Use `Access` only if the compiler wants it (remove the import otherwise). Check the real `access::access` return type and whether `Access` is needed; if `node_in_link`-style trash hiding is in `access::access` already (CLAUDE.md says it hides trashed nodes), nothing more is needed.

- [ ] **Step 6: Routes and api types.** Add the api.rs types above (derive `Clone` on `CreateReportRequest` and `ReportBox`). In `routes/mod.rs`: `pub mod reports;` and in the authenticated router:

```rust
.route("/report-keys", get(reports::keys))
.route("/reports", post(reports::create_route))
```

- [ ] **Step 7: Run the test**

Run: `cargo test -p thencloud-server --test e2e reports_by_share_recipients`
Expected: PASS.

- [ ] **Step 8: Clippy, fmt, commit**

```bash
cargo clippy --workspace --all-targets && cargo fmt --all
git add crates/thencloud-server crates/thencloud-crypto/src/api.rs
git commit -m "Let share recipients report a file to the admins

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 3: Reporting from a public link

**Files:**
- Modify: `crates/thencloud-server/src/routes/public.rs`
- Modify: `crates/thencloud-server/src/routes/mod.rs`
- Test: `crates/thencloud-server/tests/e2e.rs` (new test `reports_from_public_links`)

**Interfaces:**
- Consumes: `reports::create`, `reports::admin_keys`, `Reporter::Link { ip }`, `ClientIp::key()`.
- Produces: `GET /api/public/{token}/report-keys`, `POST /api/public/{token}/reports`.

- [ ] **Step 1: Write the failing test**

```rust
#[tokio::test]
async fn reports_from_public_links() {
    let h = Harness::new().await;
    let _admin = register(&h, "root", "admin pw").await;
    let alice = register(&h, "alice", "alice pw").await;
    let (folder, folder_key) = alice.mkdir(&h, &alice.root, "Public").await;
    let file = alice.upload(&h, &folder, None, "a.bin", b"payload").await.unwrap();
    let file_key = c::unwrap_node_key(&folder_key, &file.enc_key, &file.id).unwrap();

    // A link with a password: nothing without the link token.
    let (req, secret) = link_with_password(&folder, &folder_key, "pw");
    let link: Link = h.call(Method::POST, "/api/links", Some(&alice.token), Some(req)).await.json();
    let base = format!("/api/public/{}", link.token);
    let r = h.raw(Method::GET, &format!("{base}/report-keys"), None, &[], Body::empty(), None).await;
    assert_eq!(r.status, StatusCode::UNAUTHORIZED);
    let u: UnlockLinkResponse = h
        .call(Method::POST, &format!("{base}/unlock"), None, Some(unlock_body(&secret, "pw")))
        .await
        .json();
    let lt = [("x-link-token", u.link_token.as_str())];
    let boxes = report_boxes(&h, &format!("{base}/report-keys"), None, &lt, &file, &file_key, "").await;
    let body = |reason| serde_json::to_vec(&report_req(&file, reason, boxes.clone())).unwrap();
    let post = |headers: Vec<(&'static str, String)>, b: Vec<u8>| {
        let uri = format!("{base}/reports");
        let h = &h;
        async move {
            let hs: Vec<(&str, &str)> = headers.iter().map(|(k, v)| (*k, v.as_str())).collect();
            h.raw(Method::POST, &uri, None, &hs, Body::from(b), Some("application/json")).await
        }
    };
    let with_token = vec![("x-link-token", u.link_token.clone())];
    assert_eq!(post(vec![], body(ReportReason::Illegal)).await.status, StatusCode::UNAUTHORIZED);
    assert_eq!(post(with_token.clone(), body(ReportReason::Illegal)).await.status, StatusCode::NO_CONTENT);

    // Ten reports per address per window; the eleventh is refused.
    for i in 1..10 {
        assert_eq!(post(with_token.clone(), body(ReportReason::Other)).await.status, StatusCode::NO_CONTENT, "report {i}");
    }
    let r = post(with_token.clone(), body(ReportReason::Other)).await;
    assert_eq!(r.status, StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(r.error(), "too_many_reports");

    // A file outside the link can't be reported through it.
    let other = alice.upload(&h, &alice.root, None, "b.bin", b"x").await.unwrap();
    let mut outside = report_req(&file, ReportReason::Other, boxes.clone());
    outside.node_id = other.id.clone();
    outside.version_id = other.version.unwrap().id;
    h.state.limiter.clear("report:");
    let r = post(with_token, serde_json::to_vec(&outside).unwrap()).await;
    assert_eq!(r.status, StatusCode::NOT_FOUND);
}
```

(The in-process harness has no peer address, so `ClientIp::key()` gives `"unknown"` with limits by address on; match the key used in `reports::create` when clearing — if the key comes out as `report:unknown`, clear that instead. Check a `max_opens` link too: copy the visit-token lines from `links_with_limited_opens` and assert a report without the visit token is 404 and with it 204.)

Also add the per-node cap: after clearing the limiter, file 20 open reports from a link (clear the limiter every 9) and assert the 21st is 429 `too_many_reports`.

- [ ] **Step 2: Run to see it fail**

Run: `cargo test -p thencloud-server --test e2e reports_from_public_links`
Expected: 404/405 on the new routes.

- [ ] **Step 3: Implement in `public.rs`**

```rust
pub async fn report_keys(
    State(state): State<AppState>,
    Path(token): Path<String>,
    headers: HeaderMap,
) -> Result<Json<Vec<ReportKey>>> {
    let link = resolve(&state, &token, &headers).await?;
    if link.upload_only {
        return Err(AppError::Forbidden);
    }
    Ok(Json(crate::routes::reports::admin_keys(&state.db).await?))
}

/// A visitor reports a file under the link to the admins.
pub async fn report(
    State(state): State<AppState>,
    Path(token): Path<String>,
    headers: HeaderMap,
    ip: ClientIp,
    Json(req): Json<CreateReportRequest>,
) -> Result<StatusCode> {
    let link = resolve(&state, &token, &headers).await?;
    check_id(&req.node_id, "node_id")?;
    let node = node_in_link(&state, &link, &req.node_id).await?;
    crate::routes::reports::create(
        &state,
        crate::routes::reports::Reporter::Link { ip: ip.key() },
        &node,
        req,
    )
    .await
}
```

Note: when limits by address are off (`ip.key()` is None), the key is `report:` for everyone, so the limit applies per server; that matches `unlock`'s behaviour.

Routes in `mod.rs`, next to the other public ones:

```rust
.route("/public/{token}/report-keys", get(public::report_keys))
.route("/public/{token}/reports", post(public::report))
```

- [ ] **Step 4: Run the test**

Run: `cargo test -p thencloud-server --test e2e reports_from`
Expected: PASS.

- [ ] **Step 5: Clippy, fmt, commit**

```bash
cargo clippy --workspace --all-targets && cargo fmt --all
git add crates/thencloud-server
git commit -m "Let public link visitors report a file

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 4: Admin handling, holding reported versions, cleanup

**Files:**
- Modify: `crates/thencloud-server/src/routes/reports.rs` (admin routes, `release`, `prune`, `drop_for_owner`)
- Modify: `crates/thencloud-server/src/routes/nodes.rs:328`, `routes/versions.rs:154,225,293`, `routes/uploads.rs:483` (use `reports::delete_versions`)
- Modify: `crates/thencloud-server/src/routes/admin.rs` (`delete_account`, `server_stats`), `crates/thencloud-server/src/routes/health.rs` (gauge), `crates/thencloud-crypto/src/api.rs` (`ServerStats.open_reports`, admin types)
- Modify: `crates/thencloud-server/src/audit.rs` (actions), `crates/thencloud-server/src/janitor.rs` (prune), `crates/thencloud-server/src/maintenance.rs` (`expected_chunks`)
- Test: `crates/thencloud-server/tests/e2e.rs` (`admins_open_remove_and_clear_reports`)

**Interfaces:**
- Produces (api.rs):

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdminReport {
    pub id: String,
    pub node_id: String,
    pub version_id: String,
    pub chunk_count: u32,
    pub reason: ReportReason,
    /// The reporter's username, or None for a public link's visitor
    /// (or a reporter whose account is gone).
    pub reporter: Option<String>,
    pub via_link: bool,
    pub owner: String,
    pub created_at: i64,
    pub status: String,
    /// This admin's box, if one was sealed to them.
    pub sealed: Option<B64>,
    /// Admins without a box yet (made admin after the report).
    pub missing: Vec<ReportKey>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdminReportPage { pub reports: Vec<AdminReport>, pub more: bool }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AddReportBoxes { pub boxes: Vec<ReportBox> }
```

- Routes (admin only): `GET /api/admin/reports?status=open|closed&before=<created_at>.<id>` → `AdminReportPage` (50 per page, newest first); `GET /api/admin/reports/{id}/chunks/{idx}` → encrypted chunk (open reports only); `POST /api/admin/reports/{id}/boxes` (`AddReportBoxes`) → 204; `POST /api/admin/reports/{id}/safe` → 204; `POST /api/admin/reports/{id}/remove` → 204.
- Audit actions: `report_safe`, `report_removed` (target: owner username; detail: reason).

- [ ] **Step 1: Write the failing test**

```rust
#[tokio::test]
async fn admins_open_remove_and_clear_reports() {
    let h = Harness::new().await;
    let admin = register(&h, "root", "admin pw").await;
    let alice = register(&h, "alice", "alice pw").await;
    let bob = register(&h, "bob", "bob pw").await;
    let (folder, folder_key) = alice.mkdir(&h, &alice.root, "Shared").await;
    let bad = alice.upload(&h, &folder, None, "bad.bin", &secret_payload(9000)).await.unwrap();
    let fine = alice.upload(&h, &folder, None, "fine.txt", b"harmless").await.unwrap();
    let key_of = |n: &Node| c::unwrap_node_key(&folder_key, &n.enc_key, &n.id).unwrap();
    // Share with bob (as in reports_by_share_recipients).
    let pk: UserPublicKey = h.get("/api/users/bob/public-key", &alice.token).await.json();
    h.call(Method::POST, "/api/shares", Some(&alice.token), Some(CreateShareRequest {
        node_id: folder.clone(),
        recipient: "bob".into(),
        wrapped_key: B64(c::seal_share_key(&sealing_key(&pk), &folder_key, &folder).unwrap()),
        permission: Permission::Read,
        expires_at: None,
    })).await;
    for (n, reason) in [(&bad, ReportReason::Malware), (&fine, ReportReason::Other)] {
        let boxes = report_boxes(&h, "/api/report-keys", Some(&bob.token), &[], n, &key_of(n), "").await;
        let r = h.call(Method::POST, "/api/reports", Some(&bob.token), Some(report_req(n, reason, boxes))).await;
        assert_eq!(r.status, StatusCode::NO_CONTENT);
    }

    // Only admins.
    assert_eq!(h.get("/api/admin/reports", &bob.token).await.status, StatusCode::FORBIDDEN);
    let page: AdminReportPage = h.get("/api/admin/reports", &admin.token).await.json();
    assert_eq!(page.reports.len(), 2);
    let rb = page.reports.iter().find(|r| r.node_id == bad.id).unwrap().clone();
    let rf = page.reports.iter().find(|r| r.node_id == fine.id).unwrap().clone();
    assert_eq!(rb.reporter.as_deref(), Some("bob"));
    assert_eq!(rb.owner, "alice");

    // The owner deletes the file and empties the trash: the admin still has it.
    assert_eq!(alice.delete(&h, &bad.id).await, StatusCode::NO_CONTENT);
    let r = h.call(Method::DELETE, "/api/trash", Some(&alice.token), None::<()>).await;
    assert_eq!(r.status, StatusCode::NO_CONTENT);
    let rec = c::open_report(&admin.kp, &rb.sealed.as_ref().unwrap().0, &rb.node_id, &rb.version_id).unwrap();
    let ck = Key::from_slice(&c::b64_decode(&rec.content_key).unwrap()).unwrap();
    let mut got = Vec::new();
    for i in 0..rb.chunk_count {
        let r = h.get(&format!("/api/admin/reports/{}/chunks/{i}", rb.id), &admin.token).await;
        assert_eq!(r.status, StatusCode::OK, "{r:?}");
        got.extend(c::decrypt_chunk(&ck, &rb.version_id, i, i + 1 == rb.chunk_count, &r.body).unwrap());
    }
    got.truncate(rec.size as usize);
    assert_eq!(got, secret_payload(9000));
    assert_eq!(h.get(&format!("/api/admin/reports/{}/chunks/0", rb.id), &bob.token).await.status, StatusCode::FORBIDDEN);

    // A new admin gets a box from an admin who has one.
    let carol = register(&h, "carol", "carol pw").await;
    let users: Vec<AdminUser> = h.get("/api/admin/users", &admin.token).await.json();
    let carol_id = users.iter().find(|u| u.username == "carol").unwrap().id.clone();
    h.call(Method::PATCH, &format!("/api/admin/users/{carol_id}"), Some(&admin.token), Some(json!({"is_admin": true}))).await;
    let page: AdminReportPage = h.get("/api/admin/reports", &carol.token).await.json();
    let rf2 = page.reports.iter().find(|r| r.id == rf.id).unwrap();
    assert!(rf2.sealed.is_none());
    let page: AdminReportPage = h.get("/api/admin/reports", &admin.token).await.json();
    let rf_admin = page.reports.iter().find(|r| r.id == rf.id).unwrap();
    assert_eq!(rf_admin.missing.len(), 1);
    let rec_f = c::open_report(&admin.kp, &rf_admin.sealed.as_ref().unwrap().0, &rf.node_id, &rf.version_id).unwrap();
    let to = [rf_admin.missing[0].public_key.0.clone(), rf_admin.missing[0].pq_public_key.clone().map(|k| k.0).unwrap_or_default()].concat();
    let add = AddReportBoxes { boxes: vec![ReportBox { admin_id: carol_id.clone(), sealed: B64(c::seal_report(&to, &rec_f, &rf.node_id, &rf.version_id).unwrap()) }] };
    // Not for someone who isn't an admin, or already has one.
    let wrong = AddReportBoxes { boxes: vec![ReportBox { admin_id: "00000000-0000-4000-8000-000000000000".into(), ..add.boxes[0].clone() }] };
    assert_eq!(h.call(Method::POST, &format!("/api/admin/reports/{}/boxes", rf.id), Some(&admin.token), Some(&wrong)).await.status, StatusCode::BAD_REQUEST);
    assert_eq!(h.call(Method::POST, &format!("/api/admin/reports/{}/boxes", rf.id), Some(&admin.token), Some(&add)).await.status, StatusCode::NO_CONTENT);
    assert_eq!(h.call(Method::POST, &format!("/api/admin/reports/{}/boxes", rf.id), Some(&admin.token), Some(&add)).await.status, StatusCode::BAD_REQUEST);
    let page: AdminReportPage = h.get("/api/admin/reports", &carol.token).await.json();
    assert!(page.reports.iter().find(|r| r.id == rf.id).unwrap().sealed.is_some());

    // Mark safe: closed, the file is untouched, audited.
    let post = |uri: String, t: String| { let h = &h; async move { h.call(Method::POST, &uri, Some(&t), None::<()>).await } };
    assert_eq!(post(format!("/api/admin/reports/{}/safe", rf.id), admin.token.clone()).await.status, StatusCode::NO_CONTENT);
    assert!(alice.versions(&h, &fine.id).await.len() == 1);

    // Remove: blobs of the reported version are gone, even though the node already was.
    assert!(blob_exists(&h, &rb.version_id));
    assert_eq!(post(format!("/api/admin/reports/{}/remove", rb.id), admin.token.clone()).await.status, StatusCode::NO_CONTENT);
    assert!(!blob_exists(&h, &rb.version_id));
    assert_eq!(h.get(&format!("/api/admin/reports/{}/chunks/0", rb.id), &admin.token).await.status, StatusCode::NOT_FOUND);
    // Closed reports aren't listed as open, and can't be closed twice.
    let page: AdminReportPage = h.get("/api/admin/reports", &admin.token).await.json();
    assert!(page.reports.is_empty());
    assert_eq!(post(format!("/api/admin/reports/{}/safe", rb.id), admin.token.clone()).await.status, StatusCode::NOT_FOUND);
    let closed: AdminReportPage = h.get("/api/admin/reports?status=closed", &admin.token).await.json();
    assert_eq!(closed.reports.len(), 2);
    let audit: AuditPage = h.get("/api/admin/audit", &admin.token).await.json();
    let actions: Vec<&str> = audit.entries.iter().map(|e| e.action.as_str()).collect();
    assert!(actions.contains(&"report_safe") && actions.contains(&"report_removed"));
    // The owner is told nothing: no activity entry names the removal.
}
```

Also add, in the same test or a second one, a report on a live file that is then removed while the node exists: `remove` deletes the node (`get /api/nodes/{id}` by the owner → 404) and any link to it (`GET /api/public/<token>` → 404). And one for holding against version deletion: upload a second version of a reported file, report the old one is not possible (only the node's versions), so instead: report the current version, upload a new version, then delete the old version (`DELETE /api/nodes/{id}/versions/{old}`) and assert `blob_exists(&h, old)` is still true. Finally, run `thencloud_server::maintenance::check` (as `backup_restore_and_check` does) while a report holds a deleted version and assert no orphans and no missing chunks are reported.

- [ ] **Step 2: Run to see it fail**

Run: `cargo test -p thencloud-server --test e2e admins_open_remove`
Expected: compile errors for the new types.

- [ ] **Step 3: Route blob deletion through reports.** Replace:
  - `routes/nodes.rs:328` `state.blobs.delete_versions(&ids).await;` → `crate::routes::reports::delete_versions(state, &ids).await;`
  - `routes/versions.rs:154` `state.blobs.delete_version(&v.id).await;` → `crate::routes::reports::delete_versions(&state, std::slice::from_ref(&v.id)).await;`
  - `routes/versions.rs:225` and `:293` loops `for (id, _) in &picked { state.blobs.delete_version(id).await; }` → `let ids: Vec<String> = picked.iter().map(|p| p.0.clone()).collect(); crate::routes::reports::delete_versions(state, &ids).await;`
  - `routes/uploads.rs:483` loop over `excess` → same pattern with `excess`.
  - Leave `uploads.rs:554` (an upload that never finished) alone.

- [ ] **Step 4: Admin routes** in `reports.rs`:

```rust
use axum::extract::{Path, Query};
use axum::response::Response;
use crate::audit::{self, action};
use crate::routes::nodes::{chunk_response, delete_subtree};

const PAGE: i64 = 50;
/// Closed reports are kept this long, then deleted.
const KEEP_CLOSED_SECS: i64 = 90 * 86400;

fn require_admin(user: &AuthUser) -> Result<()> {
    if user.is_admin { Ok(()) } else { Err(AppError::Forbidden) }
}

#[derive(serde::Deserialize, Default)]
pub struct ListQuery {
    status: Option<String>,
    /// `<created_at>.<id>` of the last report on the previous page.
    before: Option<String>,
}

#[derive(sqlx::FromRow)]
struct Row {
    id: String,
    node_id: String,
    version_id: String,
    chunk_count: i64,
    reason: String,
    reporter: Option<String>,
    via_link: bool,
    owner: String,
    created_at: i64,
    status: String,
    sealed: Option<Vec<u8>>,
}

fn reason_of(s: &str) -> ReportReason {
    match s {
        "illegal" => ReportReason::Illegal,
        "malware" => ReportReason::Malware,
        "copyright" => ReportReason::Copyright,
        "harassment" => ReportReason::Harassment,
        _ => ReportReason::Other,
    }
}

pub async fn list(
    State(state): State<AppState>,
    user: AuthUser,
    Query(q): Query<ListQuery>,
) -> Result<Json<AdminReportPage>> {
    require_admin(&user)?;
    let open = match q.status.as_deref() {
        None | Some("open") => true,
        Some("closed") => false,
        _ => return Err(AppError::bad("status is open or closed")),
    };
    let (at, id) = match &q.before {
        None => (i64::MAX, String::new()),
        Some(c) => {
            let (a, i) = c.split_once('.').ok_or_else(|| AppError::bad("bad cursor"))?;
            (a.parse().map_err(|_| AppError::bad("bad cursor"))?, i.to_owned())
        }
    };
    let mut rows: Vec<Row> = sqlx::query_as(
        "SELECT r.id, r.node_id, r.version_id, r.chunk_count, r.reason, ru.username AS reporter, \
                r.via_link, ou.username AS owner, r.created_at, r.status, b.sealed \
         FROM reports r JOIN users ou ON ou.id = r.owner_id \
         LEFT JOIN users ru ON ru.id = r.reporter_id \
         LEFT JOIN report_boxes b ON b.report_id = r.id AND b.admin_id = ?1 \
         WHERE (r.status = 'open') = ?2 \
           AND (r.created_at < ?3 OR (r.created_at = ?3 AND r.id < ?4)) \
         ORDER BY r.created_at DESC, r.id DESC LIMIT ?5",
    )
    .bind(&user.id)
    .bind(open)
    .bind(at)
    .bind(&id)
    .bind(PAGE + 1)
    .fetch_all(&state.db)
    .await?;
    let more = rows.len() as i64 > PAGE;
    rows.truncate(PAGE as usize);
    let admins = admin_keys(&state.db).await?;
    let mut reports = Vec::with_capacity(rows.len());
    for r in rows {
        let have: HashSet<String> =
            sqlx::query_scalar("SELECT admin_id FROM report_boxes WHERE report_id = ?")
                .bind(&r.id)
                .fetch_all(&state.db)
                .await?
                .into_iter()
                .collect();
        reports.push(AdminReport {
            missing: if r.status == "open" {
                admins.iter().filter(|a| !have.contains(&a.admin_id)).cloned().collect()
            } else {
                vec![]
            },
            id: r.id,
            node_id: r.node_id,
            version_id: r.version_id,
            chunk_count: r.chunk_count as u32,
            reason: reason_of(&r.reason),
            reporter: r.reporter,
            via_link: r.via_link,
            owner: r.owner,
            created_at: r.created_at,
            sealed: (r.status == "open").then_some(r.sealed).flatten().map(B64),
            status: r.status,
        });
    }
    Ok(Json(AdminReportPage { reports, more }))
}

async fn open_report_row(state: &AppState, id: &str) -> Result<(String, String, i64, String, String)> {
    sqlx::query_as(
        "SELECT r.node_id, r.version_id, r.chunk_count, r.owner_id, r.reason FROM reports r \
         WHERE r.id = ? AND r.status = 'open'",
    )
    .bind(id)
    .fetch_optional(&state.db)
    .await?
    .ok_or(AppError::NotFound)
}

/// The reported version's chunks, for as long as the report is open. Not
/// counted against anyone's daily limit.
pub async fn chunk(
    State(state): State<AppState>,
    user: AuthUser,
    Path((id, idx)): Path<(String, u32)>,
) -> Result<Response> {
    require_admin(&user)?;
    let (_, version_id, count, _, _) = open_report_row(&state, &id).await?;
    chunk_response(&state, &version_id, count, idx, None).await
}

pub async fn add_boxes(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<String>,
    Json(req): Json<AddReportBoxes>,
) -> Result<StatusCode> {
    require_admin(&user)?;
    open_report_row(&state, &id).await?;
    let admins: HashSet<String> =
        admin_keys(&state.db).await?.into_iter().map(|a| a.admin_id).collect();
    let mut tx = state.db.begin().await?;
    for b in &req.boxes {
        if !admins.contains(&b.admin_id)
            || (b.sealed.len() != REPORT_SEALED_LEN && b.sealed.len() != REPORT_SEALED_HYBRID_LEN)
        {
            return Err(AppError::bad("not a box for an admin"));
        }
        let r = sqlx::query("INSERT INTO report_boxes (report_id, admin_id, sealed) VALUES (?, ?, ?)")
            .bind(&id)
            .bind(&b.admin_id)
            .bind(&b.sealed.0)
            .execute(&mut *tx)
            .await;
        match r {
            Err(sqlx::Error::Database(d)) if d.is_unique_violation() => {
                return Err(AppError::bad("that admin already has a box"));
            }
            r => r?,
        };
    }
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}

/// Close open reports matching `filter` (`id = ?` or `node_id = ?`) as
/// `status`, and delete the blobs of versions nothing holds any more and no
/// file still has.
async fn close(state: &AppState, by: &str, sql_filter: &str, arg: &str, status: &str) -> Result<()> {
    let versions: Vec<String> = sqlx::query_scalar(&format!(
        "UPDATE reports SET status = ?, handled_by = ?, handled_at = ? \
         WHERE status = 'open' AND {sql_filter} RETURNING version_id"
    ))
    .bind(status)
    .bind(by)
    .bind(crate::util::now())
    .bind(arg)
    .fetch_all(&state.db)
    .await?;
    release(state, &versions).await
}

/// Blobs of versions that no open report holds and no file has any more.
async fn release(state: &AppState, versions: &[String]) -> Result<()> {
    let mut gone = Vec::new();
    for v in versions {
        let live: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM file_versions WHERE id = ?")
            .bind(v)
            .fetch_one(&state.db)
            .await?;
        if live == 0 {
            gone.push(v.clone());
        }
    }
    delete_versions(state, &gone).await;
    Ok(())
}

pub async fn mark_safe(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<String>,
) -> Result<StatusCode> {
    require_admin(&user)?;
    let (_, _, _, owner_id, reason) = open_report_row(&state, &id).await?;
    close(&state, &user.username, "id = ?", &id, "safe").await?;
    let owner: String = sqlx::query_scalar("SELECT username FROM users WHERE id = ?")
        .bind(&owner_id).fetch_one(&state.db).await?;
    audit::record(&state.db, &user.username, action::REPORT_SAFE, Some(&owner), Some(reason)).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// Delete the reported file for good, with no notice to its owner, and close
/// every open report on it.
pub async fn remove(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<String>,
) -> Result<StatusCode> {
    require_admin(&user)?;
    let (node_id, _, _, owner_id, reason) = open_report_row(&state, &id).await?;
    let owner: String = sqlx::query_scalar("SELECT username FROM users WHERE id = ?")
        .bind(&owner_id).fetch_one(&state.db).await?;
    // Close first so delete_subtree doesn't keep what it deletes.
    let versions: Vec<String> = sqlx::query_scalar(
        "UPDATE reports SET status = 'removed', handled_by = ?, handled_at = ? \
         WHERE status = 'open' AND node_id = ? RETURNING version_id",
    )
    .bind(&user.username)
    .bind(crate::util::now())
    .bind(&node_id)
    .fetch_all(&state.db)
    .await?;
    if get_node(&state.db, &node_id).await?.is_some() {
        delete_subtree(&state, &node_id, &owner_id).await?;
    }
    release(&state, &versions).await?;
    audit::record(&state.db, &user.username, action::REPORT_REMOVED, Some(&owner), Some(reason)).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// The janitor: delete reports closed more than 90 days ago.
pub async fn prune(state: &AppState) -> Result<u64> {
    Ok(sqlx::query("DELETE FROM reports WHERE status != 'open' AND handled_at < ?")
        .bind(crate::util::now() - KEEP_CLOSED_SECS)
        .execute(&state.db)
        .await?
        .rows_affected())
}

/// An account is being deleted: its reports go too, with what they held.
pub async fn drop_for_owner(state: &AppState, owner_id: &str) -> Result<()> {
    let versions: Vec<String> =
        sqlx::query_scalar("DELETE FROM reports WHERE owner_id = ? RETURNING version_id")
            .bind(owner_id)
            .fetch_all(&state.db)
            .await?;
    release(state, &versions).await
}
```

Adjust to the real signatures:
  - `chunk_response` takes `payer: &str` today. Change it to `payer: Option<&str>` and skip `transfer::check`/`transfer::add` when `None`; update the two existing callers (`current_chunk`, `versions::chunk`) to pass `Some(payer)`.
  - `close` in `mark_safe` builds SQL with `format!`; the codebase uses `AssertSqlSafe` for dynamic SQL (see `db.rs`). Since only two literal filters are used, prefer inlining the `UPDATE` in `mark_safe` like `remove` does and dropping `close` entirely if the dynamic query needs `AssertSqlSafe`.
  - The janitor and account deletion: `mark_safe`'s `handled_at` uses `now()`; `created_at` uses `coarse_now()` (as other node times).

- [ ] **Step 5: Wire up.**
  - `routes/mod.rs` admin routes:

```rust
.route("/admin/reports", get(reports::list))
.route("/admin/reports/{id}/chunks/{idx}", get(reports::chunk))
.route("/admin/reports/{id}/boxes", post(reports::add_boxes))
.route("/admin/reports/{id}/safe", post(reports::mark_safe))
.route("/admin/reports/{id}/remove", post(reports::remove))
```

  - `audit.rs`: `pub const REPORT_SAFE: &str = "report_safe"; pub const REPORT_REMOVED: &str = "report_removed";` and mention reports in the module doc.
  - `admin.rs` `delete_account`: after `delete_subtree(...)` and before deleting the user row, `crate::routes::reports::drop_for_owner(state, id).await?;`.
  - `janitor.rs`: `crate::routes::reports::prune(state).await?;` next to `crate::audit::prune(state)`.
  - `ServerStats`: add `#[serde(default)] pub open_reports: i64`; in `server_stats`, `open_reports: count("SELECT COUNT(*) FROM reports WHERE status = 'open'").await?`; in `health.rs`, `gauge("open_reports", "Reports waiting for an admin.", s.open_reports);`.
  - `maintenance.rs` `expected_chunks`: after the `file_versions` loop, add the versions only reports hold:

```rust
let held: Vec<(String, i64)> = sqlx::query_as(
    "SELECT DISTINCT version_id, chunk_count FROM reports \
     WHERE status = 'open' AND version_id NOT IN (SELECT id FROM file_versions) ORDER BY version_id",
)
.fetch_all(db)
.await?;
for (id, count) in held {
    out.extend((0..count).map(|i| (id.clone(), i, None)));
}
```

  `check` then counts them as known (not orphans) and doesn't compare their total size (they aren't in `totals`). `backup` copies them.

- [ ] **Step 6: Run the tests**

Run: `cargo test -p thencloud-server --test e2e report && cargo test -p thencloud-server`
Expected: all PASS, including the existing version/trash/backup tests.

- [ ] **Step 7: Zero-knowledge scan.** In `full_lifecycle_is_zero_knowledge`, before its scan of the data directory, have bob (the share recipient there) report one of the shared files with a note that contains `MARKER`-style text, using `report_boxes`, so the scan also covers `reports` and `report_boxes`. Add the note text to the needles the scan looks for.

- [ ] **Step 8: Clippy, fmt, commit**

```bash
cargo clippy --workspace --all-targets && cargo fmt --all
git add crates
git commit -m "Let admins open reported files, remove them or mark them safe

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 5: Reporting in the web client

**Files:**
- Modify: `web/src/lib/crypto.js` (`fetchContent`, `openContent`)
- Modify: `web/src/lib/cloud.svelte.js` (`reportFile`, `sealReport`)
- Create: `web/src/components/dialogs/ReportDialog.svelte`
- Modify: `web/src/components/views/FilesView.svelte` (`menuFor`, dialog branch)
- Modify: `web/src/SharePage.svelte` (report button for single files, row action for folder links)
- Modify: `web/src/lib/messages/pl.js`, `web/src/lib/messages/de.js`

**Interfaces:**
- Produces (`crypto.js`):
  - `fetchContent({ contentKey, versionId, count, size, mime }, getChunk, onProgress) -> Promise<Blob>`; `fetchFile` becomes a wrapper that unwraps the content key and calls it.
  - `openContent({ contentKey, versionId, count, size, meta }, getChunk)` returning the same object shape as `openFile`; `openFile` becomes a wrapper.
  - `sealReportBoxes(keys, entry, note) -> [{ admin_id, sealed }]` where `keys` is the `ReportKey[]` from the server and `entry` is `{ node, key, meta }`.
- Produces (`cloud.svelte.js`): `reportFile(entry, reason, note) -> Promise<void>`.
- `ReportDialog` props: `{ entry, send: (reason, note) => Promise<void>, onclose }`.

- [ ] **Step 1: Split the content helpers in `crypto.js`.** Move the body of `fetchFile` after the content-key unwrap into `fetchContent`, and the body of `openFile` into `openContent`, so both can be used with a content key the admin got from a report:

```js
/** Decrypt a version's chunks with its content key; see fetchFile. */
export async function fetchContent({ contentKey, versionId, count, size, mime }, getChunk, onProgress) {
  const parts = [];
  for (let i = 0; i < count; i++) {
    parts.push(tc.decrypt_chunk(contentKey, versionId, i, i === count - 1, await getChunk(i)));
    onProgress?.((i + 1) / count);
  }
  // Contents are padded with zeros past the real size (see padded_size).
  const all = new Blob(parts);
  if (all.size < size) throw new Error('Decrypted size does not match the file metadata.');
  return all.slice(0, size, mime || 'application/octet-stream');
}

export async function fetchFile(node, nodeKey, getChunk, onProgress) {
  const meta = decryptMeta(nodeKey, node);
  const v = node.version;
  const contentKey = tc.unwrap_content_key(nodeKey, unb64(v.enc_content_key), node.id, v.id);
  const blob = await fetchContent({ contentKey, versionId: v.id, count: v.chunk_count, size: meta.size, mime: meta.mime }, getChunk, onProgress);
  return { blob, meta };
}
```

and likewise `openContent({ contentKey, versionId, count, meta }, getChunk)` holding the body of `openFile` (using `meta.size`), with `openFile` unwrapping and delegating. Add:

```js
/** Seal a report of `entry`'s current version to each admin (see seal_report). */
export function sealReportBoxes(keys, entry, note) {
  const v = entry.node.version;
  const contentKey = tc.unwrap_content_key(entry.key, unb64(v.enc_content_key), entry.node.id, v.id);
  const name = [...entry.meta.name].slice(0, 1000).join('');
  const record = JSON.stringify({ content_key: b64(contentKey), name, mime: entry.meta.mime ?? null, size: entry.meta.size, note });
  return keys.map((k) => {
    const pub = k.pq_public_key ? new Uint8Array([...unb64(k.public_key), ...unb64(k.pq_public_key)]) : unb64(k.public_key);
    return { admin_id: k.admin_id, sealed: b64(tc.seal_report(pub, record, entry.node.id, v.id)) };
  });
}
```

- [ ] **Step 2: `reportFile` in `cloud.svelte.js`** (import `sealReportBoxes` from `./crypto.js`), near `comments`:

```js
/** Show the admins this file's current version (sealed to each of them; see seal_report). */
export async function reportFile(entry, reason, note) {
  const keys = await api('GET', '/api/report-keys');
  await api('POST', '/api/reports', {
    body: { id: tc.new_id(), node_id: entry.node.id, version_id: entry.node.version.id, reason, boxes: sealReportBoxes(keys, entry, note) },
  });
}
```

- [ ] **Step 3: `ReportDialog.svelte`**

```svelte
<script>
  // Report a file to the server's admins. What's sent is sealed to them in
  // this browser: the server can't read the file, its name or the note.
  import { t } from '../../lib/i18n.svelte.js';
  import { toast } from '../../lib/ui.svelte.js';
  import ConfirmDialog from './ConfirmDialog.svelte';

  let { entry, send, onclose } = $props();

  let reason = $state('');
  let note = $state('');
  let agreed = $state(false);

  const reasons = $derived([
    ['illegal', t('Illegal content')],
    ['malware', t('Malware or phishing')],
    ['copyright', t('Copyright infringement')],
    ['harassment', t('Harassment or abuse')],
    ['other', t('Something else')],
  ]);

  async function submit() {
    await send(reason, note.trim());
    toast(t('Report sent to the admins'), { icon: 'flag' });
  }
</script>

<ConfirmDialog
  title={t('Report {name}', { name: entry.meta.name })}
  description={t("If you think this file breaks the rules, tell the people who run this server.")}
  confirmLabel={t('Send report')}
  danger
  disabled={!reason || !agreed}
  onconfirm={submit}
  {onclose}>
  <fieldset class="grid gap-2">
    <legend class="label mb-1">{t('Why are you reporting it?')}</legend>
    {#each reasons as [value, label] (value)}
      <label class="flex items-center gap-2 text-[13px]">
        <input type="radio" name="reason" {value} bind:group={reason} />
        {label}
      </label>
    {/each}
  </fieldset>
  <label class="label" for="report-note">{t('Anything the admins should know (optional)')}</label>
  <textarea id="report-note" class="input min-h-20" maxlength="2000" bind:value={note}></textarea>
  <label class="flex items-start gap-2 rounded-md border border-line bg-subtle p-3 text-[13px]">
    <input type="checkbox" class="mt-0.5" bind:checked={agreed} />
    <span>{t("I understand that this server's admins will be able to open this file and read my note. Nobody else will, and the file's owner isn't told who reported it.")}</span>
  </label>
</ConfirmDialog>
```

(Check `icons.js` for `flag`; if it isn't generated, add `'flag'` to `scripts/gen-icons.mjs` and run `npm run icons`. Match `.label`/`.input` usage to other dialogs, e.g. `ShareDialog.svelte`.)

- [ ] **Step 4: Menus.**
  - `FilesView.svelte` `menuFor`: for files when `!isOwner`, add before the trash separator: `...(!folder && !isOwner ? ['sep', { label: t('Report'), icon: 'flag', onclick: () => (dialog = { type: 'report', entry }) }] : []),` and in the dialog chain: `{:else if dialog?.type === 'report'}<ReportDialog entry={dialog.entry} send={(reason, note) => reportFile(dialog.entry, reason, note)} onclose={close} />` with the imports.
  - `SharePage.svelte`:
    - `let reporting = $state(null);`
    - `async function reportEntry(entry, reason, note) { const keys = await request('GET', `${base}/report-keys`, opts()); await request('POST', `${base}/reports`, { ...opts(), body: { id: tc.new_id(), node_id: entry.node.id, version_id: entry.node.version.id, reason, boxes: sealReportBoxes(keys, entry, note) } }); }`
    - In the single-file card, under the buttons: `<button type="button" class="btn btn-ghost mt-2 text-[13px] text-fg-muted" onclick={() => (reporting = here)}><Icon name="flag" /> {t('Report this file')}</button>`.
    - In a folder row's actions cell, beside Download: an icon button `<button type="button" class="btn btn-ghost h-7 px-2" aria-label={t('Report {name}', { name: entry.meta.name })} onclick={() => (reporting = entry)}><Icon name="flag" /></button>`.
    - At the bottom: `{#if reporting}<ReportDialog entry={reporting} send={(r, n) => reportEntry(reporting, r, n)} onclose={() => (reporting = null)} />{/if}`.

- [ ] **Step 5: Translations.** Add every new string above to `pl.js` and `de.js` (Polish: "Zgłoś {name}", "Wyślij zgłoszenie", "Treści niezgodne z prawem", "Złośliwe oprogramowanie lub phishing", "Naruszenie praw autorskich", "Nękanie lub nadużycie", "Coś innego", "Dlaczego to zgłaszasz?", "Co administratorzy powinni wiedzieć (opcjonalnie)", "Zgłoszenie wysłane do administratorów", "Zgłoś", "Zgłoś ten plik", the consent sentence and the description; German likewise). Run `node scripts/i18n-missing.mjs` from `web/` to see anything missed.

- [ ] **Step 6: Check**

Run: `./build.sh && cd web && npm run check && npm test`
Expected: 0 warnings, all tests pass (including i18n).

- [ ] **Step 7: Try it in the browser.** Start a server on a scratch data dir (`target/debug/thencloud-server --bind 127.0.0.1:8095 --data-dir <scratchpad>/reports-data --web-dir web/dist`), and in Chrome (dark mode; don't close the last tab) make an admin, a second user who shares a folder with a third, report a file from Shared with me and from a public link. Check the dialog in light and dark, at phone width, and the network panel for nothing but ids and base64.

- [ ] **Step 8: Commit**

```bash
git add web
git commit -m "Report a file from a share or a public link

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 6: Reports in the admin view

**Files:**
- Modify: `web/src/lib/cloud.svelte.js` (`adminReports`, `openAdminReport`, `adminReportAction`)
- Modify: `web/src/components/views/AdminView.svelte` (Reports section, audit sentences)
- Modify: `web/src/lib/messages/pl.js`, `web/src/lib/messages/de.js`

**Interfaces:**
- Consumes: `fetchContent`, `openContent` (Task 5), admin routes (Task 4).
- Produces (`cloud.svelte.js`):
  - `adminReports(status = 'open', before = null) -> { reports, more }`; each report gains `record` (the decrypted `ReportRecord`, or `null` when this admin has no box) and, as a side effect, missing admins get boxes (`POST .../boxes`), best effort.
  - `adminReportEntry(report) -> { node: { id, kind: 'file' }, key: null, meta: { name, mime, size } }` plus `fetchReport(report, onProgress)` / `openReport(report)` for `Preview`'s `fetch`/`open` props.
  - `adminReportAction(report, 'safe' | 'remove')`.

- [ ] **Step 1: `cloud.svelte.js`**

```js
/** Reports for admins, each with its record opened when this admin has a box. */
export async function adminReports(status = 'open', before = null) {
  const q = new URLSearchParams({ status });
  if (before) q.set('before', `${before.created_at}.${before.id}`);
  const page = await api('GET', `/api/admin/reports?${q}`);
  for (const r of page.reports) {
    r.record = null;
    if (!r.sealed) continue;
    try {
      r.record = JSON.parse(tc.open_report(sk, unb64(r.sealed), r.node_id, r.version_id));
    } catch {
      continue;
    }
    // Admins made after the report can't open it until someone who can seals it to them.
    if (r.missing.length) {
      const entry = { node: { id: r.node_id, version: { id: r.version_id } } };
      const boxes = r.missing.map((k) => {
        const pub = k.pq_public_key ? new Uint8Array([...unb64(k.public_key), ...unb64(k.pq_public_key)]) : unb64(k.public_key);
        return { admin_id: k.admin_id, sealed: b64(tc.seal_report(pub, JSON.stringify(r.record), entry.node.id, r.version_id)) };
      });
      api('POST', `/api/admin/reports/${r.id}/boxes`, { body: { boxes } }).catch(() => {});
    }
  }
  return page;
}

const reportContent = (r) => ({
  contentKey: unb64(r.record.content_key), versionId: r.version_id, count: r.chunk_count,
  size: r.record.size, mime: r.record.mime, meta: { name: r.record.name, mime: r.record.mime, size: r.record.size },
});
const reportChunk = (r) => (i) => api('GET', `/api/admin/reports/${r.id}/chunks/${i}`);

/** What Preview needs to show a reported file. */
export const adminReportEntry = (r) => ({ node: { id: r.id, kind: 'file' }, key: null, meta: { name: r.record.name, mime: r.record.mime, size: r.record.size } });
export const fetchReport = (r) => async (_entry, onProgress) => ({ blob: await fetchContent(reportContent(r), reportChunk(r), onProgress), meta: adminReportEntry(r).meta });
export const openReport = (r) => () => openContent(reportContent(r), reportChunk(r));
export const adminReportAction = (r, what) => api('POST', `/api/admin/reports/${r.id}/${what}`);
```

(Match `fetch`'s return shape to what `Preview` expects from the `fetch` prop: read how `Preview.svelte` calls `fetch(entry, ...)` and uses the result, and return the same shape. Because the record comes from someone else, `record.mime` is untrusted: `Preview` already types Blobs from `lib/preview.js`'s tables, never the stored MIME type, so pass the record through the same path as any shared file.)

- [ ] **Step 2: AdminView section**, first card on the page, shown when there are open reports or always with an empty state:

```svelte
<section class="card mt-6 overflow-hidden">
  <div class="grid gap-1 p-6 pb-4">
    <h2 class="text-base font-semibold tracking-tight">{t('Reports')}</h2>
    <p class="text-[13px] text-fg-muted">{t('Files people reported to the admins. Each one is sealed to you in the reporter\'s browser; the server can\'t read it.')}</p>
  </div>
  {#if !reports.reports.length}
    <p class="px-6 pb-6 text-[13px] text-fg-muted">{t('No open reports.')}</p>
  {:else}
    <ul class="divide-y divide-line border-t border-line">
      {#each reports.reports as r (r.id)}
        <li class="grid gap-1 px-6 py-3 text-[13px]">
          <div class="flex flex-wrap items-baseline justify-between gap-x-4">
            <span class="min-w-0 truncate font-medium">{r.record ? r.record.name : t('Not sealed to you yet')}</span>
            <span class="text-xs text-fg-muted"><Time ms={r.created_at * 1000} relative /></span>
          </div>
          <p class="text-fg-muted">
            {reasonLabel(r.reason)} · {t('owned by {owner}', { owner: r.owner })} · {r.reporter ? t('reported by {name}', { name: r.reporter }) : t('reported through a public link')}
          </p>
          {#if r.record?.note}<p class="whitespace-pre-wrap break-words">{r.record.note}</p>{/if}
          <div class="mt-1 flex flex-wrap gap-2">
            <button type="button" class="btn btn-secondary h-7 px-2.5" disabled={!r.record} onclick={() => (viewing = r)}><Icon name="eye" /> {t('Open')}</button>
            <button type="button" class="btn btn-secondary h-7 px-2.5" onclick={() => act(r, 'safe')}><Icon name="check" /> {t('Mark safe')}</button>
            <button type="button" class="btn btn-danger h-7 px-2.5" onclick={() => (dialog = { type: 'remove-report', report: r })}><Icon name="trash-2" /> {t('Remove file')}</button>
          </div>
        </li>
      {/each}
    </ul>
  {/if}
</section>
```

with, in the script: `let reports = $state(null); let viewing = $state(null);`, `adminReports()` added to `load()`'s `Promise.all`, a `reasonLabel(code)` returning the same five translated labels as the dialog, `async function act(r, what) { try { await adminReportAction(r, what); reports = await adminReports(); refreshAudit(); toast(what === 'safe' ? t('Marked safe') : t('File removed')); } catch (e) { toastError(e); } }`, a `ConfirmDialog` for `remove-report` (`title={t('Remove {name}?', ...)}`, description `t("It's deleted for good, with its versions and links. Its owner isn't told.")`, `danger`, `onconfirm={() => act(dialog.report, 'remove')}`), and `{#if viewing}<Preview entries={[adminReportEntry(viewing)]} start={0} fetch={fetchReport(viewing)} open={openReport(viewing)} ondownload={...} onclose={() => (viewing = null)} />{/if}` where `ondownload` fetches and `saveBlob`s. Update the file's top comment: admins still never see content, except a file someone chose to report to them.

`describe(e)` in AdminView gains `report_safe` → `t('{actor} marked a report on a file of {target} as safe', p)` and `report_removed` → `t('{actor} removed a reported file of {target}', p)`.

Stats: if the view shows `stats` cards, add "Open reports" with `stats.open_reports`.

- [ ] **Step 3: Translations** for all new strings in `pl.js` and `de.js`; run `node scripts/i18n-missing.mjs`.

- [ ] **Step 4: Check**

Run: `./build.sh && cd web && npm run check && npm test`
Expected: 0 warnings, all pass.

- [ ] **Step 5: Try it in Chrome** (as in Task 5, dark mode): as admin, open the report, preview the file, mark one safe, remove another (confirm modal), see both in Admin activity; make a second admin, reload the first admin's view, sign in as the second and open the report.

- [ ] **Step 6: Commit**

```bash
git add web
git commit -m "Show reports to admins, with the reported file

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 7: Browser test

**Files:**
- Modify: `web/e2e/helpers.js` (remember the first account, `signIn`)
- Create: `web/e2e/reports.spec.js`

- [ ] **Step 1: Helpers.** In `signUp`, when `setup` was true, write `{ username, password }` to `../target/e2e-data/e2e-admin.json` (`writeFileSync`), and export:

```js
/** The server's first account, if this run made it (signUp writes it down). */
export function firstAccount() {
  try {
    return JSON.parse(readFileSync('../target/e2e-data/e2e-admin.json', 'utf8'));
  } catch {
    return null;
  }
}

export async function signIn(page, username, password) {
  await page.goto('/');
  await page.getByRole('tab', { name: 'Sign in' }).click();
  await page.getByLabel('Username').fill(username);
  await page.getByLabel('Password', { exact: true }).fill(password);
  await page.getByRole('button', { name: 'Sign in' }).last().click();
  await expect(page.getByRole('heading', { name: 'My files' })).toBeVisible({ timeout: 60_000 });
}
```

- [ ] **Step 2: The spec**

```js
import { expect, test } from '@playwright/test';
import { watchRequests, uniqueName, signUp, signIn, upload, rowMenu, firstAccount } from './helpers.js';

const PASSWORD = 'correct horse battery staple';
const NAME = 'suspicious-invoice.pdf.exe';
const SECRET = 'REPORTED-CONTENTS-5b1e';
const NOTE = 'report note 8c2d';

test('a link visitor reports a file, and an admin removes it', async ({ page, context, browser }) => {
  const requests = watchRequests(context);
  // Make sure an admin exists and we know who it is.
  if (!firstAccount()) await signUp(page, uniqueName('admin'), PASSWORD);
  const admin = firstAccount();
  test.skip(!admin, 'needs the first account of a fresh server');

  const owner = await browser.newContext({ colorScheme: 'dark' });
  const ownerRequests = watchRequests(owner);
  const o = await owner.newPage();
  await signUp(o, uniqueName('owner'), PASSWORD);
  await upload(o, { [NAME]: SECRET });
  await rowMenu(o, NAME, 'Public link');
  const dialog = o.locator('dialog[open]');
  await dialog.getByRole('button', { name: 'Create link' }).click();
  const url = (await dialog.locator('.font-mono', { hasText: '/s/' }).first().textContent()).trim();
  const key = url.split('#')[1];

  const visitor = await browser.newContext({ colorScheme: 'dark' });
  const visitorRequests = watchRequests(visitor);
  const v = await visitor.newPage();
  await v.goto(url);
  await v.getByRole('button', { name: 'Report this file' }).click();
  const report = v.locator('dialog[open]');
  await report.getByLabel('Malware or phishing').check();
  await report.getByLabel('Anything the admins should know (optional)').fill(NOTE);
  await expect(report.getByRole('button', { name: 'Send report' })).toBeDisabled();
  await report.getByRole('checkbox').check();
  await report.getByRole('button', { name: 'Send report' }).click();
  await expect(v.getByText('Report sent to the admins')).toBeVisible();

  await signIn(page, admin.username, admin.password);
  await page.getByRole('link', { name: 'Admin' }).click();
  await expect(page.getByText(NAME)).toBeVisible();
  await expect(page.getByText(NOTE)).toBeVisible();
  await page.getByRole('button', { name: 'Open' }).first().click();
  await expect(page.locator('dialog.preview')).toBeVisible();
  await page.keyboard.press('Escape');
  await page.getByRole('button', { name: 'Remove file' }).first().click();
  await page.locator('dialog[open]').getByRole('button', { name: 'Remove file' }).click();
  await expect(page.getByText('No open reports.')).toBeVisible();

  // The link no longer opens.
  const again = await visitor.newPage();
  await again.goto(url);
  await expect(again.getByText('This link has expired or was removed')).toBeVisible();

  for (const r of [requests, ownerRequests, visitorRequests]) await r.expectNone([SECRET, NAME, NOTE, PASSWORD, key]);
  await owner.close();
  await visitor.close();
});
```

(Adjust selectors to what Tasks 5 and 6 actually render: the admin nav might be a button, not a link; check `Shell.svelte`. The preview of an `.exe` shows "no preview"; if `Open` is disabled for files without a preview, use a `.txt` name instead and keep the note check.)

- [ ] **Step 3: Run**

Run: `./build.sh && cd web && npx playwright test` (or the Docker route in `web/e2e/README.md`).
Expected: every spec passes.

- [ ] **Step 4: Commit**

```bash
git add web/e2e
git commit -m "Browser test for file reports

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 8: Documentation

**Files:**
- Modify: `README.md` (next to the downloader bullet near line 228), `CLAUDE.md` (Layout, server bullets), `MILESTONES.md` (tick "Files reports")

- [ ] **Step 1: README bullet** after the downloader one:

```markdown
- **Reports let admins open one file, by the reporter's choice.** Someone who can open a file they don't own (through a share or a public link) can report it. Their browser seals that version's key, its name and their note to each admin; the server stores only those sealed boxes and can't read them. An admin can then open that one version in their own browser and delete the file or mark it safe. The owner isn't told, and the admin log records what was done.
```

- [ ] **Step 2: CLAUDE.md**, in the server list after `routes/comments.rs`:

```markdown
  - `routes/reports.rs`: file reports (#108). A share recipient (`POST /api/reports`) or link visitor (`POST /api/public/{token}/reports`, rate limited by address) seals a `ReportRecord` (the version's content key, name, MIME type, size, note; padded to 16 KiB) to every admin with `seal_report`, bound to node and version; the server refuses a report without exactly one box per admin. While a report is open, `reports::delete_versions` keeps that version's blobs whatever deletes the file (every blob deletion goes through it), and `maintenance::expected_chunks` counts them. Admins list reports, get chunks through `/api/admin/reports/{id}/chunks/{n}`, reseal to newer admins (`boxes`), and mark safe or remove (`delete_subtree`, silent to the owner), both in the audit log. Closed reports are pruned after 90 days.
```

and in the web section, one line: `Reports: dialogs/ReportDialog.svelte from the file menu (not the owner's) and the share page; the Reports card in AdminView opens them with fetchContent/openContent in crypto.js.`

- [ ] **Step 3: MILESTONES.md.** Change the line to:

```markdown
- [x] **Files reports**: report a file from a share or a public link; the reporter's browser seals that version's key, name and a note to each admin (the server can't read it), with an acknowledgement before sending. Admins open it in their browser and delete the file (silently) or mark it safe, both in the admin log; the version is kept until then even if the owner deletes it
```

- [ ] **Step 4: Full verification**

Run: `cargo fmt --all --check && cargo clippy --workspace --all-targets && cargo test --workspace && ./build.sh && (cd web && npm run check && npm test)`
Expected: all clean and passing.

- [ ] **Step 5: Commit**

```bash
git add README.md CLAUDE.md MILESTONES.md
git commit -m "Document file reports

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```
