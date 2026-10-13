// Uploading a batch of local files, shared by FilesView (drag-and-drop,
// pick, paste) and the Android share-in flow (importShare.svelte.js):
// photo privacy, encrypted upload with progress, and quota refresh.

import { upload, refreshMe } from './cloud.svelte.js';
import { trackTransfer, errorMessage, photoDetails } from './ui.svelte.js';
import { fileInfo, hasDetails, stripFile } from './exif.js';
import { t } from './i18n.svelte.js';

/**
 * Mark the jobs ({ file, dest, label, clean? }) that have a photo with a
 * location to be cleaned on upload, per the `photoDetails` setting. In "ask"
 * mode, `ask(located)` is awaited for 'cancel' | 'keep' | 'remove'; resolves
 * to false if cancelled.
 */
export async function checkPhotos(jobs, ask) {
  if (photoDetails.value === 'keep') return true;
  const found = [];
  for (const job of jobs) {
    const info = await fileInfo(job.file).catch(() => null);
    if (hasDetails(info)) found.push({ job, info });
  }
  if (!found.length) return true;
  if (photoDetails.value === 'ask') {
    const located = found.filter((f) => f.info.gps).length;
    if (!located) return true;
    const choice = await ask(located);
    if (choice === 'cancel') return false;
    if (choice === 'keep') return true;
  }
  for (const f of found) f.job.clean = true;
  return true;
}

/**
 * Upload `jobs` ({ file, dest: { parentId, parentKey }, label }), three at a
 * time, tracked in the transfer tray; a photo marked `clean` (checkPhotos)
 * is stripped of its location first. `onDone()` runs once every job has
 * settled (success or error), before the quota is refreshed.
 */
export async function runUploadJobs(jobs, onDone) {
  if (!jobs.length) return;
  const one = async ({ file, dest, label, clean }) => {
    const job = trackTransfer('upload', label, file.size);
    try {
      if (clean) {
        file = await stripFile(file);
        job.size = file.size;
      }
      await upload(file, dest, (p) => (job.progress = p));
      job.status = 'done';
    } catch (e) {
      job.status = 'error';
      job.error = e?.code === 'quota_exceeded' ? t('Not enough storage left') : errorMessage(e);
    }
  };
  let next = 0;
  const worker = async () => {
    while (next < jobs.length) await one(jobs[next++]);
  };
  await Promise.all(Array.from({ length: Math.min(3, jobs.length) }, worker));
  await onDone?.();
  refreshMe().catch(() => {});
}
