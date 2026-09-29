// Video and audio conversion with ffmpeg compiled to WASM (@ffmpeg/core,
// GPL-2.0-or-later). Loaded on demand: the core is about 30 MB, served
// from our own origin like everything else, and cached by the browser after
// the first use. It runs single-threaded in a module worker.

import { FFmpeg } from '@ffmpeg/ffmpeg';
import coreURL from '@ffmpeg/core?url';
import wasmURL from '@ffmpeg/core/wasm?url';
import { t } from './i18n.svelte.js';

/** ffmpeg arguments per target format, tuned for speed in WASM. */
const ARGS = {
  mp4: ['-c:v', 'libx264', '-preset', 'veryfast', '-crf', '23', '-pix_fmt', 'yuv420p', '-c:a', 'aac', '-b:a', '160k', '-movflags', '+faststart'],
  mov: ['-c:v', 'libx264', '-preset', 'veryfast', '-crf', '23', '-pix_fmt', 'yuv420p', '-c:a', 'aac', '-b:a', '160k'],
  mkv: ['-c:v', 'libx264', '-preset', 'veryfast', '-crf', '23', '-c:a', 'aac', '-b:a', '160k'],
  webm: ['-c:v', 'libvpx', '-b:v', '1500k', '-deadline', 'realtime', '-cpu-used', '8', '-c:a', 'libvorbis'],
  avi: ['-c:v', 'mpeg4', '-q:v', '4', '-c:a', 'libmp3lame', '-q:a', '4'],
  gif: ['-an', '-vf', 'fps=12,scale=480:-2:flags=lanczos,split[a][b];[a]palettegen[p];[b][p]paletteuse', '-loop', '0'],
  mp3: ['-vn', '-c:a', 'libmp3lame', '-q:a', '2'],
  m4a: ['-vn', '-c:a', 'aac', '-b:a', '192k'],
  ogg: ['-vn', '-c:a', 'libvorbis', '-q:a', '5'],
  opus: ['-vn', '-c:a', 'libopus', '-b:a', '128k'],
  flac: ['-vn', '-c:a', 'flac'],
  wav: ['-vn', '-c:a', 'pcm_s16le'],
};

/**
 * Between these containers the streams can often just be copied, which
 * takes seconds instead of minutes. If that fails (codecs the target can't
 * hold), it's re-encoded.
 */
const REMUX_FROM = new Set(['mp4', 'm4v', 'mov', 'mkv']);
const REMUX_TO = new Set(['mp4', 'mov', 'mkv']);

let instance = null;
let loading = null;
let log = [];

async function ffmpeg(onStage) {
  if (instance) return instance;
  if (!loading) {
    onStage?.('loading');
    const f = new FFmpeg();
    f.on('log', ({ message }) => {
      log.push(message);
      if (log.length > 40) log.shift();
    });
    loading = f
      .load({ coreURL, wasmURL })
      .then(() => (instance = f))
      .finally(() => (loading = null));
  }
  return loading;
}

/** Stop whatever is running; the next conversion loads a fresh instance. */
function stop() {
  instance?.terminate();
  instance = null;
}

/**
 * Convert `blob` (whose file extension is `inputExt`) to `target`.
 * `onStage('loading' | 'converting')`, `onProgress(0..1)`, and `signal`
 * to cancel. Resolves to a Blob of `target.type`.
 */
export async function transcode(blob, inputExt, target, { trim, onProgress, onStage, signal } = {}) {
  const f = await ffmpeg(onStage);
  if (signal?.aborted) throw new DOMException('Cancelled', 'AbortError');
  const onAbort = () => stop();
  signal?.addEventListener('abort', onAbort, { once: true });
  const progress = ({ progress }) => onProgress?.(Math.min(1, Math.max(0, progress || 0)));
  f.on('progress', progress);

  const input = `input.${inputExt.replace(/[^a-z0-9]/gi, '') || 'bin'}`;
  const output = `output.${target.ext}`;
  try {
    onStage?.('converting');
    log = [];
    await f.writeFile(input, new Uint8Array(await blob.arrayBuffer()));
    // Seek before -i (fast), and give a duration rather than an end time,
    // since -to after a pre-input seek is measured from the new start.
    const start = trim?.start ?? null;
    const end = trim?.end ?? null;
    const cut = start != null ? ['-ss', String(start)] : [];
    const length = end != null ? ['-t', String(end - (start ?? 0))] : [];
    const run = (args) => f.exec(['-hide_banner', '-y', ...cut, '-i', input, ...length, ...args, output]);
    let code = 1;
    // Copying streams can only cut at keyframes, so trims are re-encoded.
    const trimmed = start != null || end != null;
    if (!trimmed && REMUX_FROM.has(inputExt) && REMUX_TO.has(target.id)) {
      code = await run(['-c', 'copy', ...(target.id === 'mp4' ? ['-movflags', '+faststart'] : [])]);
    }
    if (code !== 0) code = await run(ARGS[target.id]);
    if (signal?.aborted) throw new DOMException('Cancelled', 'AbortError');
    if (code !== 0) {
      const detail = log.filter((l) => /error|invalid|not supported|unknown/i.test(l)).pop();
      throw new Error(detail ? t("This file couldn't be converted: {detail}", { detail: detail.trim() }) : t("This file couldn't be converted."));
    }
    const data = await f.readFile(output);
    return new Blob([data], { type: target.type });
  } catch (e) {
    if (signal?.aborted) throw new DOMException('Cancelled', 'AbortError');
    throw e;
  } finally {
    signal?.removeEventListener('abort', onAbort);
    if (instance === f) {
      f.off('progress', progress);
      await f.deleteFile(input).catch(() => {});
      await f.deleteFile(output).catch(() => {});
    }
  }
}
