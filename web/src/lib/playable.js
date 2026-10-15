// Watching a video the browser can't play (HEVC in many browsers, some MKV
// and AVI files): the decrypted file is converted here with ffmpeg.wasm and
// played from memory. Nothing is uploaded or saved. Copying the streams
// into MP4 is tried first, which takes seconds and is enough when only the
// container was the problem; if the picture still doesn't show, it's
// re-encoded (slow: roughly real time or worse), to H.264 in MP4 where the
// browser plays that, else to VP8 in WebM.
import { fetchEntry } from './cloud.svelte.js';
import { extension } from './preview.js';
import { MAX_MEDIA } from './convert.js';

export { MAX_MEDIA };

/**
 * Whether a <video> element that loaded can show a picture: a codec the
 * browser lacks often still plays the sound over a 0 x 0 picture instead
 * of failing.
 */
export const noPicture = (el) => el.readyState >= 1 && el.videoWidth === 0;

/** Resolves to whether the browser shows a picture for `url` (within `ms`). */
export function canShow(url, ms = 15000) {
  return new Promise((resolve) => {
    const v = document.createElement('video');
    v.muted = true;
    v.preload = 'auto';
    const done = (ok) => {
      clearTimeout(timer);
      v.removeAttribute('src');
      v.load();
      resolve(ok);
    };
    const timer = setTimeout(() => done(false), ms);
    v.addEventListener('loadeddata', () => done(v.videoWidth > 0), { once: true });
    v.addEventListener('error', () => done(false), { once: true });
    v.src = url;
  });
}

/**
 * Decrypt `entry` and convert it into something this browser plays.
 * `onStage('downloading' | 'loading' | 'remuxing' | 'converting')`,
 * `onProgress(0..1)` within a stage, `signal` to cancel. Resolves to a
 * blob: URL the caller revokes.
 */
export async function convertForViewing(entry, { onStage, onProgress, signal } = {}) {
  onStage?.('downloading');
  const { blob } = await fetchEntry(entry, (p) => onProgress?.(p));
  if (signal?.aborted) throw new DOMException('Cancelled', 'AbortError');
  const { transcode } = await import('./ffmpeg.js');
  const ext = extension(entry.meta.name);
  const stage = (s) => (s === 'loading' ? onStage?.('loading') : null);

  // Copying the streams into MP4 first, when the container is one ffmpeg can copy from.
  if (['mkv', 'mov', 'm4v', 'mp4'].includes(ext)) {
    onStage?.('remuxing');
    try {
      const copied = await transcode(blob, ext, { id: 'mp4', ext: 'mp4', type: 'video/mp4' }, { onStage: stage, signal, onProgress });
      const url = URL.createObjectURL(copied);
      if (await canShow(url)) return url;
      URL.revokeObjectURL(url);
    } catch (e) {
      if (signal?.aborted) throw e;
      // Not copyable: re-encode below.
    }
  }
  onStage?.('converting');
  const h264 = document.createElement('video').canPlayType('video/mp4; codecs="avc1.42E01E, mp4a.40.2"') !== '';
  const target = h264 ? { id: 'view', ext: 'mp4', type: 'video/mp4' } : { id: 'viewWebm', ext: 'webm', type: 'video/webm' };
  const out = await transcode(blob, ext, target, { onStage: stage, signal, onProgress, copy: false });
  return URL.createObjectURL(out);
}
