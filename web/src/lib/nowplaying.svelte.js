// What the music player is playing, for the Android app to show in a
// notification, on the lock screen and to headphone buttons, and to keep
// playing while the app is in the background (NowPlaying.kt in
// crates/thencloud-app/android-plugin). Android's WebView has
// navigator.mediaSession but shows nothing for it.
//
// Names and the cover go to the system, on this device only; they show on the
// lock screen like any music app's.

import { untrack } from 'svelte';
import { base64, call, nativeSaves } from './native.js';

const ARTWORK = 512;

let shown = false;
let last = null;
let position = () => 0;

/**
 * Follow `state()` (null when nothing is loaded, else title, artist, album,
 * cover, duration, rate, playing and buffering) and `pos()` (seconds), and
 * hand the system's buttons to `actions`, which take what
 * navigator.mediaSession's action handlers take.
 */
export function showNowPlaying(state, pos, actions) {
  if (!nativeSaves) return;
  position = pos;
  // Called by the plugin; false tells it there's no player here any more.
  globalThis.thencloudMedia = (details) => {
    const handler = actions[details?.action];
    if (!handler || !last) return false;
    handler(details);
    return true;
  };
  addEventListener('pagehide', () => shown && call('media_stop').catch(() => {}));
  $effect.root(() => {
    $effect(() => {
      const s = state();
      untrack(() => send(s));
    });
    let cover;
    $effect(() => {
      const url = state()?.cover ?? null;
      if (url === cover) return;
      cover = url;
      artwork(url).then(
        (data) => url === cover && call('media_artwork', { data }).then(() => last && send(last)),
        () => {},
      );
    });
  });
}

/** After a seek, or anything else that moves the position without changing the state. */
export function syncNowPlaying() {
  if (last) send(last);
}

function send(s) {
  last = s;
  if (!s) {
    if (shown) call('media_stop').catch(() => {});
    shown = false;
    return;
  }
  shown = true;
  const { title, artist, album, duration, rate, playing, buffering } = s;
  call('media_update', { title, artist, album, duration: duration || 0, position: position() || 0, rate, playing, buffering }).catch((e) => console.error(e));
}

/** The cover scaled down and drawn again as a JPEG, so the system only ever decodes ours. */
async function artwork(url) {
  if (!url) return null;
  const img = new Image();
  img.src = url;
  await img.decode();
  const scale = Math.min(1, ARTWORK / Math.max(img.naturalWidth, img.naturalHeight));
  const canvas = document.createElement('canvas');
  canvas.width = Math.max(1, Math.round(img.naturalWidth * scale));
  canvas.height = Math.max(1, Math.round(img.naturalHeight * scale));
  canvas.getContext('2d').drawImage(img, 0, 0, canvas.width, canvas.height);
  const blob = await new Promise((resolve) => canvas.toBlob(resolve, 'image/jpeg', 0.9));
  return blob ? base64(new Uint8Array(await blob.arrayBuffer())) : null;
}
