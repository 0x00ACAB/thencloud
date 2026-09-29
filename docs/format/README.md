# thencloud formats

This is every ciphertext, key derivation and encoding a thencloud client has
to get right to read and write the same data as the web client and the CLI.
The code is `crates/thencloud-crypto/src/lib.rs`; this describes what it does,
so another client can be written without reading Rust.

[`vectors.json`](vectors.json) has test vectors for all of it. The Rust crate
(`crates/thencloud-crypto/tests/vectors.rs`) and the WASM build
(`web/tests/vectors.test.js`) are both checked against it on every CI run.
A new client should pass it too.

## Conventions

- **Bytes on the wire and in links** are base64url without padding
  (RFC 4648 §5). Decoders drop trailing `=` first, so padded input is accepted.
- **Strings** are UTF-8. Passwords are put in Unicode NFC before Argon2,
  so `ä` typed as one character or as `a` plus a combining accent is the same
  password. The vectors have both spellings.
- **Ids** (users, nodes, versions, app passwords) are lowercase UUID strings.
  Node and version ids are chosen by the client, since they're bound into
  ciphertexts before the server has seen them.
- **Times** are milliseconds since the Unix epoch inside encrypted
  metadata, and seconds (rounded down to the hour) on the server.
- `||` is concatenation. `HKDF(salt, ikm, info)` is HKDF-SHA256 with a
  32-byte output. `SHA256(x)` is the full 32-byte digest.

## The AEAD

Everything symmetric is XChaCha20-Poly1305 with a 256-bit key and a fresh
random 24-byte nonce:

```
sealed = nonce (24) || ciphertext || tag (16)
```

Anything shorter than 40 bytes fails to open. Every use binds its context as
associated data:

```
aad(label, parts...) = "thencloud/v1/" || label || for each part: 0x00 || part
```

So `aad("node-key", id)` is `thencloud/v1/node-key\0<id>`, and a label with
no parts is just `thencloud/v1/master-key`. The server can store these
ciphertexts but can't move one to another node, version, user or position:
it no longer opens there.

## Keys

```text
password ──Argon2id──► root ──HKDF──┬─► auth_key   (sent to the server, hashed again there)
                                    └─► kek        (never leaves the client)
kek ──wraps──► master key (MK)
MK  ──wraps──► X25519 secret, ML-KEM-768 seed, root folder key, private data
folder key ──wraps──► child node keys (files and folders)
node key ──seals──► node metadata
file node key ──wraps──► a content key per version ──seals──► content chunks
recipient's public key ──sealed box──► node keys for shares and file drops
```

All symmetric keys are 32 random bytes.

### From a password

```
root     = Argon2id v1.3 (NFC(password), salt, m_cost KiB, t_cost, p_cost) → 32 bytes
auth_key = HKDF(salt: none, ikm: root, info: "thencloud/v1/auth")
kek      = HKDF(salt: none, ikm: root, info: "thencloud/v1/kek")
```

The salt is 16 random bytes. Salt and parameters are stored on the server in
the clear and returned by prelogin. New accounts use `m_cost 65536, t_cost 3,
p_cost 1`. The server accepts 19456 to 1048576 KiB, `t_cost` 2 to 10 and
`p_cost` 1 to 4. The server hashes `auth_key` again (Argon2id, its own salt)
before storing it, so what it holds can't be replayed.

### Recovery keys

A recovery key is 32 random bytes. It needs no slow KDF:

```
auth_key = HKDF(salt: "thencloud/v1/recovery", ikm: key, info: "thencloud/v1/recovery-auth")
kek      = HKDF(salt: "thencloud/v1/recovery", ikm: key, info: "thencloud/v1/recovery-kek")
```

It's written down as Crockford base32 (`0123456789ABCDEFGHJKMNPQRSTVWXYZ`)
of `key || check`, where `check` is the first 2 bytes of
`SHA256("thencloud/v1/recovery-check\0" || key)`. The 272 bits make 55
characters, most significant bit first, with the last character padded with
zero bits. They're printed in groups of five joined by `-`. When reading one
back, spaces and dashes are ignored, case doesn't matter, `O` reads as `0` and
`I`/`L` as `1`. Anything that isn't 34 bytes with a matching check is refused.

### App passwords

These are also 32 random bytes, shown like recovery keys:

```
auth_key = HKDF(salt: "thencloud/v1/app-password", ikm: key, info: "thencloud/v1/app-password-auth")
kek      = HKDF(salt: "thencloud/v1/app-password", ikm: key, info: "thencloud/v1/app-password-kek")
```

### Passkeys

The WebAuthn PRF extension is evaluated at the fixed input
`SHA256("thencloud/v1/passkey-prf")`. Its output (at least 32 bytes) becomes a
KEK:

```
kek = HKDF(salt: "thencloud/v1/passkey", ikm: prf_output, info: "thencloud/v1/passkey-kek")
```

### Link passwords

A public link with a password carries a random 32-byte `secret` after the
`#`, not the node key. The password and the secret together make the keys:

```
salt     = first 16 bytes of SHA256("thencloud/v1/link-salt\0" || secret)
a        = the password keys above, from (password, salt, default parameters)
auth_key = HKDF(salt: none,   ikm: a.auth_key, info: "thencloud/v1/link-auth")
kek      = HKDF(salt: secret, ikm: a.kek,      info: "thencloud/v1/link-kek")
```

The visitor sends `auth_key` to unlock the link (the server keeps only its
hash), and gets back the node key wrapped under `kek`. The salt comes from
the secret, so the server can't guess passwords before it has seen the link,
and a server that skips the check still hands out nothing it or the visitor
can open without the password. For an upload-only link, `secret` is the
owner's identity from the link. Nothing is wrapped there; the password only
gates uploads.

### Key pairs

- **X25519**: the secret is 32 random bytes, used as is. The public key is the
  usual base-point multiplication.
- **ML-KEM-768** (FIPS 203): the secret is the 64-byte seed `d || z`, from
  which `ML-KEM.KeyGen_internal` makes the 1184-byte encapsulation key.
  Accounts made before post-quantum keys have none until their next sign-in.
- **Sealing key**: what others seal to. It's the X25519 public key alone, or
  `x25519_public || mlkem_public` (1216 bytes) when there's an ML-KEM key.
- **Identity**: what fingerprints cover and upload-only links carry. It's
  `x25519_public`, or `x25519_public || SHA256(mlkem_public)` (64 bytes).
- **Fingerprint**: the first 16 bytes of `SHA256(identity)` as lowercase hex,
  in eight groups of four joined by spaces: `1a2b 3c4d ...`.

The web client keeps its secret as the X25519 secret followed by the ML-KEM
seed (32 or 96 bytes). The vectors' sealed boxes use that layout for
`secret`.

## Wrapped keys and private data

Each of these is `seal(key, plaintext, aad)`:

| Format | Key | Plaintext | Associated data |
|---|---|---|---|
| `master-key` | password KEK | MK | `aad("master-key")` |
| `master-key-recovery` | recovery KEK | MK | `aad("master-key-recovery")` |
| `master-key-app` | app password KEK | MK | `aad("master-key-app", app_password_id)` |
| `master-key-passkey` | passkey KEK | MK | `aad("master-key-passkey", base64url(credential_id))` |
| `private-key` | MK | X25519 secret | `aad("private-key")` |
| `pq-private-key` | MK | ML-KEM seed (64) | `aad("pq-private-key")` |
| `node-key` | parent folder's key, or MK for a root folder | node key | `aad("node-key", node_id)` |
| `content-key` | file's node key | content key | `aad("content-key", node_id, version_id)` |
| `private-data` | MK | any bytes | `aad("private-data", user_id, label)` |
| `avatar` | the user's avatar key | the image | `aad("avatar", owner_username)` |
| `person-details` | the user's avatar key | pronouns and grammatical gender as JSON, zero-padded to 256 bytes (see below) | `aad("person-details", owner_username)` |
| `display-name` | the user's avatar key | the display name, UTF-8, zero-padded to 256 bytes (see below) | `aad("display-name", owner_username)` |
| `link-key` | a link password's KEK | node key | `aad("link-key", node_id)` |
| `link-secret` | the node key | the link's secret, so the owner can show the link again | `aad("link-secret", node_id)` |
| `thumbnail` | the file's node key | a small JPEG of that version | `aad("thumbnail", node_id, version_id)` |
| `comment` | the node key | a comment: JSON `{"text": "...", "at": <ms>}` | `aad("comment", node_id, comment_id, author_user_id)` |
| `backup` | a backup key | one backup record (see [Backups](#backups)) | `aad("backup", base64url(backup_id), decimal(index))` |

A display name is NFC, trimmed, 1 to 64 characters, with no control
characters and none of U+061C, U+200E, U+200F, U+202A to U+202E and U+2066
to U+2069 (bidirectional formatting, which could make one name read as
another). The padding hides its length. A reader must refuse a name that
isn't already in that form, or whose padding isn't all zeros, since any
client could have written it; clients show it next to the username, never
instead of it.

Person details are how someone likes to be referred to: JSON with optional
`subject`, `object` and `possessive` pronouns (English: "they", "them",
"their"; each NFC, trimmed, at most 24 characters, with the same forbidden
characters as display names) and an optional `gender` (`neuter`, `feminine`
or `masculine`), for languages whose words change with it. Missing means
not said. The JSON is padded like a display name, and a reader refuses
pronouns not already in stored form, an unknown gender or non-zero padding.

Private data labels in use: `contacts` (verified contacts), `avatar-key` (the
owner's copy of their avatar key), `music`, `videos`, `files`, `notes`,
`books`, `search`, `health` (the Health module's measurements and moods) and
`prefs` (which optional modules are on) (app data, JSON) and
`draft:<node id>` (unsaved text). The server keeps
each under its label and user, and the associated data stops it from handing
one back as another.

App data is UTF-8 JSON followed by ASCII spaces up to `padded_size` of its
length (the same buckets as file contents), but never past the server's
limit for that label less 64 bytes; readers parse it as JSON, which ignores
the spaces. Data written before the padding has none and reads the same.

## Node metadata

Everything the server must not know about a node is JSON:

```json
{"name":"Holiday photos.zip","mime":"application/zip","size":1234,"mtime":1790000000000,"changed":1790000123456}
{"name":"IMG_2041.jpg","mime":"image/jpeg","size":3145728,"mtime":1790000000000,"changed":1790000123456,"taken":1563120239000}
```

- `name` (required) must not be empty, `.` or `..`, and must not contain `/`
  or NUL. At most 1024 bytes.
- `mime` is optional and only a hint. The web client never trusts it for
  rendering.
- `size` is the real plaintext size in bytes (0 for folders).
- `mtime` is the file's own modification time.
- `changed` is when the node was last made, uploaded or renamed. It's missing
  on older items, whose time comes from the server (to the hour).
- `taken` (optional) is when a photo was taken, from its EXIF
  `DateTimeOriginal` (else `DateTime`) at upload, read as the uploader's
  local time. It's only there when the uploaded file still had it.

Readers must ignore fields they don't know. Before sealing, the JSON is
padded with spaces (valid JSON whitespace) to a multiple of 128 bytes, so the
length says little about the name:

```
enc_metadata = seal(node_key, json || spaces, aad("metadata", node_id))
```

## Name tags

The server refuses a second node with the same name in a folder without
learning names. It does this by comparing a tag:

```
tag = HKDF(salt: "thencloud/v1/name-index", ikm: folder_key, info: NFC(lowercase(name)))
```

`lowercase` is Unicode's full lowercase mapping, the same in every locale
(Rust's `str::to_lowercase`). So `Café.txt` and `CAFE` plus a combining accent
and `.TXT` get the same tag. Tags differ per folder and mean nothing without
the folder key. The name itself is stored as typed.

## File content

A file version's plaintext is padded with zeros to `padded_size(size)` and
cut into 4 MiB (4194304-byte) chunks. The real size is in the metadata;
readers drop the zeros after it.

```
padded_size(size):             # Padmé: at most about 12% larger
    L = max(size, 256)
    E = floor(log2(L))
    S = floor(log2(E)) + 1
    mask = 2^(E - S) - 1
    return (L + mask) & ~mask

chunk_count(n) = max(1, ceil(n / 4194304))
```

An empty file is still one chunk, so a file can't be truncated to nothing.
Each chunk is sealed under the version's content key:

```
chunk_i = seal(content_key, piece_i, aad("chunk", version_id, decimal(i), "last" if i is the final chunk else "more"))
```

Chunks can't be reordered, swapped between versions, or dropped from the end:
a file cut short has no chunk marked `last`. An encrypted chunk is at most
4194344 bytes.

## Sealed boxes

These send a key to another user's public key without either side being
online together. They're used for shares, file drops and avatar keys.

**To an X25519-only key** (32 bytes):

```
eph     = random X25519 secret
shared  = X25519(eph, recipient_x25519)
k       = HKDF(salt: eph_pub || recipient_x25519, ikm: shared, info: "thencloud/v1/sealed-box")
box     = eph_pub (32) || seal(k, plaintext, aad)
```

**To an X25519 + ML-KEM-768 key** (1216 bytes). This stays closed as long as
either algorithm holds:

```
eph               = random X25519 secret
x_shared          = X25519(eph, recipient_x25519)
(ct, pq_shared)   = ML-KEM-768.Encaps(recipient_mlkem)            # ct is 1088 bytes
k   = HKDF(salt: eph_pub || recipient_x25519 || SHA256(ct) || SHA256(recipient_mlkem),
           ikm:  x_shared || pq_shared,
           info: "thencloud/v2/sealed-box-hybrid")
box = 0x02 || eph_pub (32) || ct (1088) || seal(k, plaintext, aad)
```

A reader treats a box as hybrid when it starts with `0x02` and is at least
1161 bytes long. Every payload is a 32-byte key, so a classic box is always
shorter than that. A hybrid box can't be opened without the ML-KEM key.

| Format | Plaintext | Associated data |
|---|---|---|
| `share` | node key | `aad("share", node_id)` |
| `drop` | a dropped file's node key, sealed to the folder's owner | `aad("drop", node_id, folder_id)` |
| `avatar-key` | the owner's avatar key | `aad("avatar-key", owner_username, grantee_username)` |

## Public links

A link is `/s/<token>#<base64url(node key)>`, or with a password
`/s/<token>#p.<base64url(secret)>` (see [Link passwords](#link-passwords)).
Browsers never send what follows `#`, so the key stays on the client. The
token only lets the visitor fetch the node's ciphertexts. A client must
never put the key, the secret or the password in a path, query string,
header, body or log. A `p.` link that the server serves without asking for
the password is refused: it has nothing that opens.

An upload-only link carries the owner's **identity** after the `#` instead
of a node key. That's 32 bytes, or 64 when the owner has an ML-KEM key. With
64, the page fetches the ML-KEM key from the server and refuses to send
anything unless its SHA-256 matches the second half. Each dropped file gets a
random node key, which is sealed to the owner as a `drop` box.

## Backups

`thencloud backup` writes a folder into one file under a random 32-byte
backup key. The key is shown once, in the same text format as a recovery
key. The backup holds the files decrypted and sealed again, so it can be
restored into any account on any server:

```
file   = "thncbk01" || id (16 random bytes) || record*
record = length (u32, big-endian) || seal(backup_key, plain, aad("backup", base64url(id), decimal(index)))
plain  = 'E' || entry JSON   {"path": ["Docs", "a.txt"], "folder": false, "size": 5, "mtime": 0, "mime": "text/plain"}
       | 'D' || bytes        the next bytes of the last file entry, at most 4 MiB
       | 'Z'                 the end
```

Records are numbered from 0. `path` runs from the folder that was backed up
(not included) to the item's own name, and a folder's entry comes before
anything in it. A file entry is followed by data records holding exactly
`size` bytes. A reader stops at the first record that doesn't open, a
missing end record, or anything after it, so a backup can't be cut short,
reordered or spliced with another without it showing.

## Wire types

The JSON the server speaks is `crates/thencloud-crypto/src/api.rs`. Binary
fields there are base64url strings: `enc_key` (a wrapped node key),
`enc_metadata`, `enc_content_key`, `name_tag`, `public_key`, and so on.
Chunks are uploaded and fetched as raw bytes at
`/api/uploads/<id>/chunks/<i>` and `/api/nodes/<id>/chunks/<i>`.

## Versions

Associated data starts with `thencloud/v1/`. Of the key derivation labels,
only the hybrid sealed box uses `v2`, since it came later. A format that
changes gets a new label, so old and new ciphertexts can't be confused. The
vectors in this directory must keep passing: new formats add vectors and
never replace old ones.
