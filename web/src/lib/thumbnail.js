// Thumbnails, made in the browser when a file is uploaded and encrypted
// under the file's key (see `uploadThumbnail` in cloud.svelte.js). The
// server only keeps the ciphertext. Images are drawn as they are, videos
// from a frame a little way in, PDFs from their first page. All of it is
// the uploader's own file, read locally: nothing is fetched.

/** Longest side, in pixels: enough for a grid tile on a sharp screen. */
const SIZE = 256;
/** Images and PDFs bigger than this aren't decoded just for a thumbnail. */
const MAX_IMAGE = 30 * 1024 * 1024;
const MAX_PDF = 50 * 1024 * 1024;
/** What the server accepts, less room for the encryption. */
const MAX_BYTES = 60 * 1024;
/** How long a video may take to show a frame. */
const VIDEO_WAIT = 10_000;

const IMAGE_TYPES = new Set(['image/jpeg', 'image/png', 'image/webp', 'image/gif', 'image/avif', 'image/bmp']);
const VIDEO_TYPES = new Set(['video/mp4', 'video/webm', 'video/quicktime', 'video/ogg', 'video/x-matroska']);

function kind(file) {
  if (!file.size) return null;
  if (IMAGE_TYPES.has(file.type) && file.size <= MAX_IMAGE) return 'image';
  if (VIDEO_TYPES.has(file.type)) return 'video';
  if (file.type === 'application/pdf' && file.size <= MAX_PDF) return 'pdf';
  return null;
}

export const canThumbnail = (file) => kind(file) !== null;

/** A small JPEG of `file`, or null if the browser can't draw it. */
export async function makeThumbnail(file) {
  try {
    switch (kind(file)) {
      case 'image':
        return await fromImage(file);
      case 'video':
        return await fromVideo(file);
      case 'pdf':
        return await fromPdf(file);
      default:
        return null;
    }
  } catch {
    return null;
  }
}

/** Draw `source` (width × height) scaled down, as a JPEG within the size limit. */
async function toJpeg(source, width, height) {
  if (!width || !height) return null;
  const scale = Math.min(1, SIZE / Math.max(width, height));
  const w = Math.max(1, Math.round(width * scale));
  const h = Math.max(1, Math.round(height * scale));
  const canvas = new OffscreenCanvas(w, h);
  const ctx = canvas.getContext('2d');
  // JPEG has no transparency: put see-through images on white.
  ctx.fillStyle = '#fff';
  ctx.fillRect(0, 0, w, h);
  ctx.drawImage(source, 0, 0, w, h);
  for (const quality of [0.8, 0.6, 0.4]) {
    const blob = await canvas.convertToBlob({ type: 'image/jpeg', quality });
    if (blob.size <= MAX_BYTES) return new Uint8Array(await blob.arrayBuffer());
  }
  return null;
}

async function fromImage(file) {
  const bitmap = await createImageBitmap(file, { imageOrientation: 'from-image' });
  try {
    return await toJpeg(bitmap, bitmap.width, bitmap.height);
  } finally {
    bitmap.close();
  }
}

/** Resolves on `event`, rejects on an error or after `ms`. */
function once(el, event, ms) {
  return new Promise((resolve, reject) => {
    const done = (fn) => (e) => {
      clearTimeout(timer);
      el.removeEventListener(event, ok);
      el.removeEventListener('error', fail);
      fn(e);
    };
    const ok = done(resolve);
    const fail = done(() => reject(new Error(`the video couldn't be read`)));
    const timer = setTimeout(fail, ms);
    el.addEventListener(event, ok);
    el.addEventListener('error', fail);
  });
}

async function fromVideo(file) {
  const url = URL.createObjectURL(file);
  const video = document.createElement('video');
  video.muted = true;
  video.playsInline = true;
  video.preload = 'auto';
  try {
    video.src = url;
    await once(video, 'loadeddata', VIDEO_WAIT);
    // A little way in, past black lead-ins, but not far into a long film.
    const d = video.duration;
    video.currentTime = Number.isFinite(d) && d > 0 ? Math.min(d * 0.1, 5) : 1;
    await once(video, 'seeked', VIDEO_WAIT);
    return await toJpeg(video, video.videoWidth, video.videoHeight);
  } finally {
    video.removeAttribute('src');
    video.load();
    URL.revokeObjectURL(url);
  }
}

async function fromPdf(file) {
  const { openPdf } = await import('./pdf.js');
  const task = openPdf(new Uint8Array(await file.arrayBuffer()));
  try {
    const doc = await task.promise;
    const page = await doc.getPage(1);
    const base = page.getViewport({ scale: 1 });
    const viewport = page.getViewport({ scale: SIZE / Math.max(base.width, base.height) });
    const canvas = document.createElement('canvas');
    canvas.width = Math.floor(viewport.width);
    canvas.height = Math.floor(viewport.height);
    await page.render({ canvas, viewport }).promise;
    return await toJpeg(canvas, canvas.width, canvas.height);
  } finally {
    task.destroy();
  }
}

/** Whether decrypted bytes are a JPEG, as every thumbnail we make is. */
export const isJpeg = (b) => b.length > 3 && b[0] === 0xff && b[1] === 0xd8 && b[2] === 0xff;
