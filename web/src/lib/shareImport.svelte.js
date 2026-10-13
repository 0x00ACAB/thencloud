// Files shared in from another app ("Share to thencloud" in Android's share
// sheet): crates/thencloud-app/android-plugin queues them natively (it's the
// only side that can read a content: Uri) and the page lists them here.
import { inAndroidApp, pendingShares } from './native.js';

export const shareImport = $state({ items: [] }); // [{ id, name, mime, size }]

/** Check for shared files waiting to be imported: once at sign-in, and again whenever the plugin says one just arrived. */
export async function refreshPendingShares() {
  if (!inAndroidApp) return;
  try {
    shareImport.items = await pendingShares();
  } catch (e) {
    console.error('Could not list shared files.', e);
  }
}

if (inAndroidApp) globalThis.thencloudShareArrived = () => refreshPendingShares();
