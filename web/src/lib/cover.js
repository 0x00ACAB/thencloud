// Album covers and video posters: an image picked here is scaled down and
// uploaded, encrypted like any file, into the folder it belongs to
// (cover.jpg, poster.jpg), so the music and video scans find it there.

import { t } from './i18n.svelte.js';
import { keyOf, upload } from './cloud.svelte.js';

/** `file` as a JPEG no larger than `max` pixels on its longest side. */
export async function scaleImage(file, max = 1200) {
  const bitmap = await createImageBitmap(file);
  const s = Math.min(1, max / Math.max(bitmap.width, bitmap.height));
  const [w, h] = [Math.round(bitmap.width * s), Math.round(bitmap.height * s)];
  const canvas = new OffscreenCanvas(w, h);
  canvas.getContext('2d').drawImage(bitmap, 0, 0, w, h);
  bitmap.close();
  return canvas.convertToBlob({ type: 'image/jpeg', quality: 0.88 });
}

/**
 * Upload `file` as `name` in a folder: a new version of `existing` when that
 * is the same file, else a new file. Resolves to { entry, blob }.
 */
export async function putFolderImage(folderId, name, file, existing) {
  const blob = await scaleImage(file);
  const f = new File([blob], name, { type: 'image/jpeg', lastModified: Date.now() });
  const same = existing?.parentId === folderId && existing.meta.name.toLowerCase() === name.toLowerCase();
  const up = await upload(f, same ? { existing } : { parentId: folderId, parentKey: await keyOf(folderId) });
  return { entry: { ...up, parentId: folderId }, blob };
}

/** A friendlier error for an image the browser couldn't decode. */
export const imageError = (e) => (e?.name === 'InvalidStateError' || e?.name === 'EncodingError' ? new Error(t("That image couldn't be read.")) : e);
