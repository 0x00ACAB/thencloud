// Thumbnails, made in the browser when an image is uploaded and encrypted
// under the file's key (see `uploadThumbnail` in cloud.svelte.js). The
// server only keeps the ciphertext.

/** Longest side, in pixels: enough for a grid tile on a sharp screen. */
const SIZE = 256;
/** Images bigger than this aren't decoded just for a thumbnail. */
const MAX_SOURCE = 30 * 1024 * 1024;
/** What the server accepts, less room for the encryption. */
const MAX_BYTES = 60 * 1024;

const IMAGE_TYPES = new Set(['image/jpeg', 'image/png', 'image/webp', 'image/gif', 'image/avif', 'image/bmp']);

export const canThumbnail = (file) => IMAGE_TYPES.has(file.type) && file.size > 0 && file.size <= MAX_SOURCE;

/** A small JPEG of `file`, or null if the browser can't draw it. */
export async function makeThumbnail(file) {
  if (!canThumbnail(file)) return null;
  let bitmap;
  try {
    bitmap = await createImageBitmap(file, { imageOrientation: 'from-image' });
  } catch {
    return null;
  }
  try {
    const scale = Math.min(1, SIZE / Math.max(bitmap.width, bitmap.height));
    const w = Math.max(1, Math.round(bitmap.width * scale));
    const h = Math.max(1, Math.round(bitmap.height * scale));
    const canvas = new OffscreenCanvas(w, h);
    const ctx = canvas.getContext('2d');
    // JPEG has no transparency: put see-through images on white.
    ctx.fillStyle = '#fff';
    ctx.fillRect(0, 0, w, h);
    ctx.drawImage(bitmap, 0, 0, w, h);
    for (const quality of [0.8, 0.6, 0.4]) {
      const blob = await canvas.convertToBlob({ type: 'image/jpeg', quality });
      if (blob.size <= MAX_BYTES) return new Uint8Array(await blob.arrayBuffer());
    }
    return null;
  } catch {
    return null;
  } finally {
    bitmap.close();
  }
}

/** Whether decrypted bytes are a JPEG, as every thumbnail we make is. */
export const isJpeg = (b) => b.length > 3 && b[0] === 0xff && b[1] === 0xd8 && b[2] === 0xff;
