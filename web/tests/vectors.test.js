// The WASM build against docs/format/vectors.json, the same file the Rust
// crate is checked against (crates/thencloud-crypto/tests/vectors.rs).
// Skipped until ../build.sh has built src/wasm.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { existsSync, readFileSync } from 'node:fs';

const wasmDir = new URL('../src/wasm/', import.meta.url);
const built = existsSync(new URL('thencloud_wasm_bg.wasm', wasmDir));
const vectors = JSON.parse(readFileSync(new URL('../../docs/format/vectors.json', import.meta.url), 'utf8'));

let tc;
if (built) {
  tc = await import(new URL('thencloud_wasm.js', wasmDir).href);
  tc.initSync({ module: readFileSync(new URL('thencloud_wasm_bg.wasm', wasmDir)) });
}
const skip = built ? false : 'the WASM build is missing; run ../build.sh';

const bytes = (s) => tc.b64_decode(s);
const b64 = (b) => tc.b64_encode(b);

// Only the CLI unwraps these; the browser only makes them.
const noBinding = new Set(['master-key-app']);

/** Opens a symmetric vector the way the web client does. */
function openSymmetric(v) {
  const [k, c, sealed] = [bytes(v.key), v.context, bytes(v.sealed)];
  switch (v.format) {
    case 'master-key': return tc.unwrap_master_key(k, sealed);
    case 'master-key-recovery': return tc.unwrap_master_key_recovery(k, sealed);
    case 'master-key-passkey': return tc.unwrap_master_key_passkey(bytes(v.prf_output), sealed, bytes(c[0]));
    case 'private-key': return tc.unwrap_private_key(k, sealed);
    case 'pq-private-key': return tc.unwrap_pq_private_key(k, sealed);
    case 'private-data': return tc.decrypt_private_data(k, c[0], c[1], sealed);
    case 'avatar': return tc.decrypt_avatar(k, c[0], sealed);
    case 'node-key': return tc.unwrap_node_key(k, sealed, c[0]);
    case 'metadata': {
      assert.deepEqual(JSON.parse(tc.decrypt_metadata(k, c[0], sealed)), v.metadata);
      return null;
    }
    case 'content-key': return tc.unwrap_content_key(k, sealed, c[0], c[1]);
    case 'chunk': return tc.decrypt_chunk(k, c[0], Number(c[1]), c[2] === 'last', sealed);
    default: throw new Error(`unknown format ${v.format}`);
  }
}

function openBox(v) {
  const [s, c, sealed] = [bytes(v.secret), v.context, bytes(v.sealed)];
  switch (v.format) {
    case 'share': return tc.open_share_key(s, sealed, c[0]);
    case 'drop': return tc.open_drop_key(s, sealed, c[0], c[1]);
    case 'avatar-key': return tc.open_avatar_key(s, sealed, c[0], c[1]);
    default: throw new Error(`unknown format ${v.format}`);
  }
}

function checkAll(list, open) {
  for (const v of list.filter((v) => !noBinding.has(v.format))) {
    const what = `${v.format} ${JSON.stringify(v.context)}${v.why ? ` (${v.why})` : ''}`;
    if (v.valid === false) {
      assert.throws(() => open(v), undefined, `should not open: ${what}`);
      continue;
    }
    const got = open(v);
    if (got) assert.equal(b64(got), v.plaintext, what);
  }
}

test('vectors: keys derived from passwords, recovery keys, app passwords and passkeys', { skip }, () => {
  for (const v of vectors.account_keys) {
    const k = tc.derive_account_keys(v.password, bytes(v.salt), JSON.stringify(v.params));
    assert.equal(b64(k.auth_key), v.auth_key);
    assert.equal(b64(k.kek), v.kek);
    k.free();
  }
  for (const v of vectors.recovery_keys) {
    assert.equal(tc.encode_recovery_key(bytes(v.key)), v.text);
    for (const typed of v.also_accepted) assert.equal(b64(tc.decode_recovery_key(typed)), v.key);
    const d = tc.derive_recovery_keys(bytes(v.key));
    assert.deepEqual([b64(d.auth_key), b64(d.kek)], [v.auth_key, v.kek]);
  }
  for (const v of vectors.app_password_keys) {
    const d = tc.derive_app_password_keys(bytes(v.key));
    assert.deepEqual([b64(d.auth_key), b64(d.kek)], [v.auth_key, v.kek]);
  }
  for (const v of vectors.passkeys) assert.equal(b64(tc.passkey_prf_salt()), v.prf_salt);
});

test('vectors: public keys, identities and fingerprints', { skip }, () => {
  for (const v of vectors.keypairs) {
    assert.equal(b64(tc.public_key_from_secret(bytes(v.x25519_secret))), v.x25519_public);
    const pq = v.pq_seed ? tc.pq_public_key_from_seed(bytes(v.pq_seed)) : new Uint8Array();
    if (v.pq_seed) assert.equal(b64(pq), v.pq_public);
    const id = tc.identity(bytes(v.x25519_public), pq);
    assert.equal(b64(id), v.identity);
    assert.equal(tc.fingerprint(id), v.fingerprint);
  }
});

test('vectors: name tags and padding', { skip }, () => {
  for (const v of vectors.name_tags) assert.equal(b64(tc.name_tag(bytes(v.folder_key), v.name)), v.tag, v.name);
  for (const v of vectors.padding) {
    assert.equal(tc.padded_size(v.size), v.padded, `size ${v.size}`);
    assert.equal(tc.chunk_count(v.padded), v.chunks, `size ${v.size}`);
  }
});

test('vectors: every ciphertext opens under its context, and only there', { skip }, () => {
  checkAll(vectors.symmetric, openSymmetric);
  checkAll(vectors.sealed_boxes, openBox);
});
