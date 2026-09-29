// "Keep me signed in on this browser": an opt-in exception to keys living
// only in memory.
//
// The session token and master key are saved in IndexedDB, encrypted with
// an AES-GCM key that is itself stored there as a *non-extractable*
// WebCrypto key: the browser uses it but won't hand its bytes to any script,
// ours included. On disk the saved keys are then as safe as the browser
// profile and the disk it's on, which is what the user is choosing to trust.
// (While the app is open, code running in the page can still use the keys,
// exactly as it can when they're only in memory.)
//
// It's cleared on sign-out, and as soon as the server says the session is
// gone (signed out elsewhere, account disabled, password reset).
//
// This is local storage wrapping only; the protocol's crypto stays in
// thencloud-crypto.

const DB = 'thencloud';
const STORE = 'kv';
const KEY = 'remembered-session';

function open() {
  return new Promise((resolve, reject) => {
    const r = indexedDB.open(DB, 1);
    r.onupgradeneeded = () => r.result.createObjectStore(STORE);
    r.onsuccess = () => resolve(r.result);
    r.onerror = () => reject(r.error);
  });
}

async function run(mode, action) {
  const db = await open();
  try {
    return await new Promise((resolve, reject) => {
      const tx = db.transaction(STORE, mode);
      const req = action(tx.objectStore(STORE));
      tx.oncomplete = () => resolve(req?.result);
      tx.onerror = () => reject(tx.error);
      tx.onabort = () => reject(tx.error);
    });
  } finally {
    db.close();
  }
}

const enc = new TextEncoder();
const aad = (userId) => enc.encode(`thencloud/v1/remembered-session\0${userId}`);
const toB64 = (bytes) => btoa(String.fromCharCode(...bytes));
const fromB64 = (s) => Uint8Array.from(atob(s), (c) => c.charCodeAt(0));

/** Save the session for next time. */
export async function rememberSession({ userId, token, masterKey }) {
  const key = await crypto.subtle.generateKey({ name: 'AES-GCM', length: 256 }, false, ['encrypt', 'decrypt']);
  const iv = crypto.getRandomValues(new Uint8Array(12));
  const plain = enc.encode(JSON.stringify({ token, mk: toB64(masterKey) }));
  const data = await crypto.subtle.encrypt({ name: 'AES-GCM', iv, additionalData: aad(userId) }, key, plain);
  plain.fill(0);
  await run('readwrite', (s) => s.put({ v: 1, userId, key, iv, data, savedAt: Date.now() }, KEY));
}

/** Whether a session is saved, without opening it. */
export async function hasRememberedSession() {
  try {
    return !!(await run('readonly', (s) => s.get(KEY)));
  } catch {
    return false;
  }
}

/** The saved session, or null. A record that won't decrypt is removed. */
export async function rememberedSession() {
  let rec;
  try {
    rec = await run('readonly', (s) => s.get(KEY));
  } catch {
    return null; // IndexedDB unavailable (private mode, blocked storage)
  }
  if (!rec || rec.v !== 1) return null;
  try {
    const plain = new Uint8Array(await crypto.subtle.decrypt({ name: 'AES-GCM', iv: rec.iv, additionalData: aad(rec.userId) }, rec.key, rec.data));
    const { token, mk } = JSON.parse(new TextDecoder().decode(plain));
    plain.fill(0);
    return { userId: rec.userId, token, masterKey: fromB64(mk) };
  } catch {
    await forgetSession();
    return null;
  }
}

export async function forgetSession() {
  try {
    await run('readwrite', (s) => s.delete(KEY));
  } catch {
    /* nothing saved, or storage unavailable */
  }
}
