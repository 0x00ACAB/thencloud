const DB = 'thencloud-file-cache';
const VERSION = 2;
const MAX_CACHE_BYTES = 512 * 1024 * 1024;

function open() {
  return new Promise((resolve, reject) => {
    const r = indexedDB.open(DB, VERSION);
    r.onupgradeneeded = () => {
      if (!r.result.objectStoreNames.contains('files')) r.result.createObjectStore('files', { keyPath: 'id' });
      const chunks = r.result.objectStoreNames.contains('chunks')
        ? r.transaction.objectStore('chunks')
        : r.result.createObjectStore('chunks', { keyPath: ['fileId', 'versionId', 'index'] });
      if (!chunks.indexNames.contains('byFile')) chunks.createIndex('byFile', 'fileId');
    };
    r.onsuccess = () => resolve(r.result);
    r.onerror = () => reject(r.error);
  });
}

async function run(stores, mode, action) {
  const db = await open();
  try {
    return await new Promise((resolve, reject) => {
      const tx = db.transaction(stores, mode);
      const result = action(tx);
      tx.oncomplete = () => resolve(result?.result);
      tx.onerror = () => reject(tx.error);
      tx.onabort = () => reject(tx.error);
    });
  } finally {
    db.close();
  }
}

export const cacheId = (server, user, node) => `${server}\0${user}\0${node}`;

export function getCachedFile(id) {
  return run(['files'], 'readonly', (tx) => tx.objectStore('files').get(id));
}

export function getCachedChunk(fileId, versionId, index) {
  return run(['chunks'], 'readonly', (tx) => tx.objectStore('chunks').get([fileId, versionId, index]));
}

export function saveCachedChunk(file, index, hash, bytes) {
  if (file.node.version.size > MAX_CACHE_BYTES) return Promise.resolve(false);
  return run(['files', 'chunks'], 'readwrite', (tx) => {
    const files = tx.objectStore('files');
    const chunks = tx.objectStore('chunks');
    const previous = files.get(file.id);
    previous.onsuccess = () => {
      const oldFile = previous.result;
      const sameVersion = oldFile?.node.version.id === file.node.version.id;
      const oldChunk = chunks.get([file.id, file.node.version.id, index]);
      oldChunk.onsuccess = () => {
        const oldSize = sameVersion ? oldChunk.result?.bytes?.byteLength ?? 0 : 0;
        const cachedBytes = Math.max(0, (sameVersion ? oldFile.cachedBytes ?? 0 : 0) - oldSize) + bytes.byteLength;
        files.put({ ...file, cachedBytes, lastUsed: Date.now() });

        const cursor = chunks.index('byFile').openCursor(IDBKeyRange.only(file.id));
        cursor.onsuccess = () => {
          const row = cursor.result;
          if (!row) return;
          if (row.value.versionId !== file.node.version.id) row.delete();
          row.continue();
        };
        chunks.put({
          fileId: file.id,
          versionId: file.node.version.id,
          index,
          hash,
          bytes: bytes.buffer.slice(bytes.byteOffset, bytes.byteOffset + bytes.byteLength),
        });

        const all = files.getAll();
        all.onsuccess = () => {
          let total = all.result.reduce((sum, row) => sum + (row.cachedBytes ?? 0), 0);
          const oldest = all.result
            .sort((a, b) => (a.lastUsed ?? a.savedAt) - (b.lastUsed ?? b.savedAt));
          for (const row of oldest) {
            if (total <= MAX_CACHE_BYTES) break;
            total -= row.cachedBytes ?? 0;
            files.delete(row.id);
            const oldChunks = chunks.index('byFile').openCursor(IDBKeyRange.only(row.id));
            oldChunks.onsuccess = () => {
              const item = oldChunks.result;
              if (!item) return;
              item.delete();
              item.continue();
            };
          }
        };
      };
    };
    return true;
  });
}

export function deleteCachedChunk(fileId, versionId, index) {
  return run(['chunks'], 'readwrite', (tx) => tx.objectStore('chunks').delete([fileId, versionId, index]));
}

export async function sha256Hex(bytes) {
  const digest = new Uint8Array(await crypto.subtle.digest('SHA-256', bytes));
  return Array.from(digest, (byte) => byte.toString(16).padStart(2, '0')).join('');
}

export function canRefresh({ connected, metered, dataSaver }) {
  return connected && !metered && !dataSaver;
}

export function sameHashes(a, b) {
  return Array.isArray(a) && Array.isArray(b) && a.length === b.length && a.every((hash, i) => hash === b[i]);
}

export function chooseFileSnapshot(cached, current, refresh) {
  if (!current) return cached ? 'cached-unverified' : 'unavailable';
  if (cached?.version.id === current.version.id) {
    return cached.enc_key === current.enc_key ? 'current' : 'invalid';
  }
  if (cached && !refresh && cached.enc_key === current.enc_key) return 'cached-stale';
  return 'current';
}
