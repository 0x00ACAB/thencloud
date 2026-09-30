# File reports (#108)

## Goal

Let people who can open a file they didn't upload report it to the server's
admins, and let an admin remove it or mark it safe, without weakening the
rule that no key, password or plaintext reaches the server.

## Decisions

- **Who reports:** visitors of a public link (`/s/<token>`) and people a file
  was shared with (directly or through a shared folder). Owners can't report
  their own files.
- **What the admin gets:** the reporter's browser seals a small record to each
  admin's public key: the reported version's content key, the file's name and
  MIME type, and the reporter's note. The admin decrypts the existing
  ciphertext in their own browser. Nothing is re-uploaded, only that one
  version is revealed, and the server holds sealed boxes only.
- **Consent:** the report dialog has a checkbox the reporter must tick,
  saying the admins will be able to open this file and read the note.
- **Remove:** a permanent delete (`delete_subtree`), which also ends any
  public links to it. The owner is not told. The audit log records it.
- **Mark safe:** closes the report. The audit log records it.

## Crypto (`thencloud-crypto`)

- `ReportRecord { content_key, name, mime, note }`, JSON, padded to a Padmé
  bucket, with the note capped at 2,000 characters.
- `seal_report(admin_pub, record, node_id, version_id)` and
  `open_report(kp, sealed, node_id, version_id)`: `seal_to_public` with a new
  sealed-box kind byte, and associated data binding the label `report`, the
  node id and the version id. Hybrid (X25519 + ML-KEM) when the admin has an
  ML-KEM key, as elsewhere.
- WASM wrappers in `thencloud-wasm`. A `docs/format/README.md` entry and
  vectors in `tests/vectors.rs` (`vectors.json` regenerated; the format is
  unreleased).

## Server (`thencloud-server`)

Migration `0026_reports.sql`:

- `reports`: `id`, `node_id`, `version_id`, `owner_id`, `reason` (one of
  `illegal`, `malware`, `copyright`, `harassment`, `other`), `reporter_id`
  (NULL for a link visitor), `link_id` (NULL for a signed-in reporter),
  `created_at` (coarse, `util::coarse_now`), `status` (`open`, `safe`,
  `removed`), `handled_by` (username text, like the audit log), `handled_at`.
- `report_boxes`: `report_id`, `admin_id`, `sealed`, primary key
  `(report_id, admin_id)`, deleted with the report or the admin.

Routes (`routes/reports.rs`):

- `GET /api/report-keys` (signed in) and `GET /api/public/{token}/report-keys`:
  each admin's id and sealing key. Not usernames.
- `POST /api/reports`: `{ node_id, version_id, reason, boxes: [{admin_id,
  sealed}] }`. Needs read access through `access::require`, refuses the owner,
  and needs the version to be the node's current one or one of its versions.
  Needs a box for every current admin, so none is left out silently.
- `POST /api/public/{token}/reports`: the same, for a node within the link
  (`access::is_within`), with the visit token when the link has `max_opens`,
  and the link password already checked as for downloads.
- Limits: one open report per (node, reporter) for signed-in users; for link
  visitors, rate limited by `ClientIp` (never stored) and at most 20 open
  reports per node. 429 `too_many_reports` past either.
- Admin only (`is_admin`):
  - `GET /api/admin/reports?status=open&before=<id>`: a page of reports
    (id, node id, version id, reason, reporter username or "link", owner
    username, created time, whether this admin has a box).
  - `GET /api/admin/reports/{id}`: the report plus this admin's box and the
    version's chunk count and size.
  - `GET /api/admin/reports/{id}/chunks/{n}`: the version's ciphertext chunks,
    only while the report is open, through `chunk_response` (not counted
    against the owner's transfer limit).
  - `POST /api/admin/reports/{id}/boxes`: add boxes for admins who lack one
    (resealing for a newly made admin).
  - `POST /api/admin/reports/{id}/safe` and `.../remove`: in one transaction
    with `audit::record` (`report_safe`, `report_removed`, with the owner's
    username as target and the reason as value). Removing closes every open
    report on that node as `removed`.
- While a report is open, its version and node are kept from permanent
  deletion: the trash purge, the janitor, version pruning and a user's
  `delete_subtree` skip them (the owner's trash still hides the node from the
  owner). Deleting a whole account still deletes everything, and closes its
  reports.
- The janitor deletes closed reports after 90 days.
- `/api/metrics` and the admin stats gain an open reports count.

## Web client

- `ReportDialog.svelte`: reason (radio list), optional note, the consent
  checkbox, Send. Reached from the file menu in `SharePage.svelte` (files in
  a folder link and single-file links) and in Shared with me / files inside a
  shared folder that the user doesn't own.
- `reportFile` in `cloud.svelte.js` and the share page's equivalent: fetch
  the report keys, seal the record to each, post.
- `AdminView.svelte` gets a Reports tab: open reports, newest first; opening
  one decrypts the record, shows name, reason, note and the file in
  `Preview` (with the Blob typed by `lib/preview.js`, as always), plus
  Download, Mark safe and Remove (confirm modal). If some admins lack a box,
  the view seals the record to them quietly.
- All text in English, Polish and German.

## Documentation

- README: next to the downloader's bullet, say that a report lets the admins
  open that one file version, by the reporter's choice, and the server still
  sees only sealed data.
- CLAUDE.md: `routes/reports.rs` in the layout; the tick in MILESTONES.md.

## Tests

- Crypto: seal/open round trip; a box doesn't open under another node or
  version id; vectors.
- Server e2e: report through a share and through a link (with password and
  `max_opens`); owner and strangers refused; missing admin boxes refused;
  limits; non-admins refused on admin routes; chunks only while open; remove
  deletes blobs and ends the link; safe; an open report survives the owner's
  trash purge; reports and boxes in the zero-knowledge scan (no name, note or
  content in the database or blob store).
- Fuzz: the report JSON goes through the existing `api-json` target.
- Playwright (`web/e2e/reports.spec.js`): a link visitor reports, an admin
  opens it and removes it; `watchRequests` sees no name, note or key.

## Out of scope

- Hashing reported files against known-abuse lists.
- Telling the reporter what happened.
- Reporting whole folders (report the files in them).
