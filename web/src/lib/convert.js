// "Convert to": change a file's format in the browser. The file is already
// decrypted here; the result is downloaded or uploaded as a new encrypted
// file, so the server never sees either.
//
// Images use the browser's own decoders and encoders (plus a tiny BMP
// writer). Video and audio use ffmpeg compiled to WASM (lib/ffmpeg.js),
// loaded only the first time it's needed.

import { previewKind, extension } from './preview.js';

/** @typedef {{ id: string, label: string, ext: string, type: string, kind: 'image' | 'video' | 'audio', quality?: boolean }} Target */

const IMAGE_TARGETS = [
  { id: 'png', label: 'PNG', ext: 'png', type: 'image/png', kind: 'image' },
  { id: 'jpeg', label: 'JPEG', ext: 'jpg', type: 'image/jpeg', kind: 'image', quality: true },
  { id: 'webp', label: 'WebP', ext: 'webp', type: 'image/webp', kind: 'image', quality: true },
  { id: 'avif', label: 'AVIF', ext: 'avif', type: 'image/avif', kind: 'image', quality: true },
  { id: 'bmp', label: 'BMP', ext: 'bmp', type: 'image/bmp', kind: 'image' },
];

const VIDEO_TARGETS = [
  { id: 'mp4', label: 'MP4', ext: 'mp4', type: 'video/mp4', kind: 'video' },
  { id: 'webm', label: 'WebM', ext: 'webm', type: 'video/webm', kind: 'video' },
  { id: 'mkv', label: 'MKV', ext: 'mkv', type: 'video/x-matroska', kind: 'video' },
  { id: 'mov', label: 'MOV', ext: 'mov', type: 'video/quicktime', kind: 'video' },
  { id: 'avi', label: 'AVI', ext: 'avi', type: 'video/x-msvideo', kind: 'video' },
  { id: 'gif', label: 'GIF', ext: 'gif', type: 'image/gif', kind: 'video' },
];

const AUDIO_TARGETS = [
  { id: 'mp3', label: 'MP3', ext: 'mp3', type: 'audio/mpeg', kind: 'audio' },
  { id: 'm4a', label: 'M4A (AAC)', ext: 'm4a', type: 'audio/mp4', kind: 'audio' },
  { id: 'ogg', label: 'Ogg Vorbis', ext: 'ogg', type: 'audio/ogg', kind: 'audio' },
  { id: 'opus', label: 'Opus', ext: 'opus', type: 'audio/ogg', kind: 'audio' },
  { id: 'flac', label: 'FLAC', ext: 'flac', type: 'audio/flac', kind: 'audio' },
  { id: 'wav', label: 'WAV', ext: 'wav', type: 'audio/wav', kind: 'audio' },
];

/** Media inputs ffmpeg reads that the preview tables don't list. */
const EXTRA_VIDEO = new Set(['avi', 'mkv', 'flv', 'wmv', 'mpg', 'mpeg', '3gp', 'ts', 'm2ts']);
const EXTRA_AUDIO = new Set(['wma', 'aiff', 'aif', 'amr', 'ac3']);

/** Everything is decrypted and converted in memory; WASM ffmpeg has 2 GB in total. */
export const MAX_IMAGE = 200 * 1024 * 1024;
export const MAX_MEDIA = 700 * 1024 * 1024;

/** What `meta` is, for conversion: 'image', 'video', 'audio' or null. */
export function sourceKind(meta) {
  const ext = extension(meta.name);
  const kind = previewKind(meta)?.kind;
  if (kind === 'image' && ext !== 'svg' && ext !== 'ico') return 'image';
  if (kind === 'video' || EXTRA_VIDEO.has(ext)) return 'video';
  if (kind === 'audio' || EXTRA_AUDIO.has(ext)) return 'audio';
  const mime = (meta.mime || '').split('/')[0];
  return ['image', 'video', 'audio'].includes(mime) && ext !== 'svg' ? mime : null;
}

// Which image types this browser can encode (Chrome can't write AVIF yet,
// for example). Checked once, lazily.
let encodable = null;
async function imageEncoders() {
  if (encodable) return encodable;
  const canvas = new OffscreenCanvas(1, 1);
  canvas.getContext('2d');
  const ok = new Set(['bmp']); // written by hand below
  for (const t of IMAGE_TARGETS) {
    if (t.id === 'bmp') continue;
    try {
      const blob = await canvas.convertToBlob({ type: t.type });
      if (blob.type === t.type) ok.add(t.id);
    } catch {
      /* not supported */
    }
  }
  return (encodable = ok);
}

/**
 * Formats `meta` can be converted to, grouped: [{ title, targets }]. Its own
 * format is left out. Video can also become audio (the soundtrack) or a GIF.
 */
export async function targetsFor(meta) {
  const kind = sourceKind(meta);
  const ext = extension(meta.name);
  const same = (t) => t.ext === ext || (t.id === 'jpeg' && ext === 'jpeg') || (t.id === 'opus' && ext === 'opus');
  if (kind === 'image') {
    const ok = await imageEncoders();
    return [{ title: 'Image', targets: IMAGE_TARGETS.filter((t) => ok.has(t.id) && !same(t)) }];
  }
  if (kind === 'video') {
    return [
      { title: 'Video', targets: VIDEO_TARGETS.filter((t) => !same(t)) },
      { title: 'Audio only', targets: AUDIO_TARGETS },
    ];
  }
  if (kind === 'audio') return [{ title: 'Audio', targets: AUDIO_TARGETS.filter((t) => !same(t)) }];
  return [];
}

/** "holiday.mov" + mp4 -> "holiday.mp4" */
export function convertedName(name, target) {
  const dot = name.lastIndexOf('.');
  return `${dot > 0 ? name.slice(0, dot) : name}.${target.ext}`;
}

/**
 * Convert `blob`. `quality` is 0..1 for lossy image formats. Reports
 * progress 0..1 and can be stopped with `signal`. Resolves to a Blob.
 */
export async function convert(blob, meta, target, { quality = 0.9, onProgress, onStage, signal } = {}) {
  if (target.kind === 'image' && sourceKind(meta) === 'image') return convertImage(blob, target, quality);
  const { transcode } = await import('./ffmpeg.js');
  return transcode(blob, extension(meta.name) || 'bin', target, { onProgress, onStage, signal });
}

async function convertImage(blob, target, quality) {
  const bitmap = await createImageBitmap(blob);
  const canvas = new OffscreenCanvas(bitmap.width, bitmap.height);
  const ctx = canvas.getContext('2d');
  // JPEG and BMP have no transparency; put it on white instead of black.
  if (target.id === 'jpeg' || target.id === 'bmp') {
    ctx.fillStyle = '#ffffff';
    ctx.fillRect(0, 0, canvas.width, canvas.height);
  }
  ctx.drawImage(bitmap, 0, 0);
  bitmap.close();
  if (target.id === 'bmp') return encodeBmp(ctx.getImageData(0, 0, canvas.width, canvas.height));
  return canvas.convertToBlob({ type: target.type, quality: target.quality ? quality : undefined });
}

/** 24-bit uncompressed BMP, bottom-up rows padded to 4 bytes. */
function encodeBmp({ width, height, data }) {
  const row = Math.ceil((width * 3) / 4) * 4;
  const size = 54 + row * height;
  const buf = new ArrayBuffer(size);
  const v = new DataView(buf);
  v.setUint8(0, 0x42);
  v.setUint8(1, 0x4d); // "BM"
  v.setUint32(2, size, true);
  v.setUint32(10, 54, true); // pixel data offset
  v.setUint32(14, 40, true); // BITMAPINFOHEADER
  v.setInt32(18, width, true);
  v.setInt32(22, height, true);
  v.setUint16(26, 1, true); // planes
  v.setUint16(28, 24, true); // bits per pixel
  v.setUint32(34, row * height, true);
  const out = new Uint8Array(buf);
  for (let y = 0; y < height; y++) {
    const dst = 54 + (height - 1 - y) * row;
    for (let x = 0; x < width; x++) {
      const s = (y * width + x) * 4;
      const d = dst + x * 3;
      out[d] = data[s + 2];
      out[d + 1] = data[s + 1];
      out[d + 2] = data[s];
    }
  }
  return new Blob([buf], { type: 'image/bmp' });
}
