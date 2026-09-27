// Runs Argon2id (thencloud-crypto via WASM) off the main thread.

import init, { derive_account_keys } from '../wasm/thencloud_wasm.js';

const ready = init();

self.onmessage = async ({ data }) => {
  const { id, password, salt, params } = data;
  try {
    await ready;
    const k = derive_account_keys(password, salt, params);
    const msg = { id, authKey: k.auth_key, kek: k.kek };
    k.free();
    self.postMessage(msg);
  } catch (e) {
    self.postMessage({ id, error: String(e?.message || e) });
  }
};
