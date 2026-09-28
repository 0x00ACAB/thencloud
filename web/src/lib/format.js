import { previewKind } from './preview.js';
import { formatDateTime, relativeTime } from './locale.svelte.js';

/**
 * When a node last changed, in ms: the exact time from its encrypted
 * metadata, or for items from before that was kept, the server's (which
 * is only to the hour).
 */
export const changedAt = (entry) => entry.meta?.changed ?? entry.node.updated_at * 1000;

export function formatSize(n) {
  if (n == null) return '';
  if (n < 1024) return `${n} B`;
  const units = ['KB', 'MB', 'GB', 'TB'];
  let i = -1;
  do {
    n /= 1024;
    i++;
  } while (n >= 1024 && i < units.length - 1);
  return `${n < 10 ? n.toFixed(1) : Math.round(n)} ${units[i]}`;
}

/** "just now", "5 minutes ago", "yesterday", then a plain date. */
export function formatWhen(ms) {
  if (!ms) return '';
  const s = Math.round((ms - Date.now()) / 1000);
  const abs = Math.abs(s);
  if (abs < 45) return 'just now';
  if (abs < 3600) return relativeTime(Math.round(s / 60), 'minute');
  if (abs < 86400) return relativeTime(Math.round(s / 3600), 'hour');
  if (abs < 7 * 86400) return relativeTime(Math.round(s / 86400), 'day');
  return formatDate(ms);
}

export function formatDate(ms) {
  return ms ? formatDateTime(ms, { day: 'numeric', month: 'short', year: 'numeric' }) : '';
}

export function fullDate(ms) {
  return ms ? formatDateTime(ms, { dateStyle: 'long', timeStyle: 'short' }) : '';
}

/** Pick an icon for a file from its MIME type or extension. */
export function fileIcon(meta) {
  const mime = meta.mime || '';
  const ext = (meta.name.split('.').pop() || '').toLowerCase();
  const kind = previewKind(meta)?.kind;
  if (kind === 'image' || mime.startsWith('image/')) return 'file-image';
  if (kind === 'video' || mime.startsWith('video/')) return 'file-video';
  if (kind === 'audio' || mime.startsWith('audio/')) return 'file-audio';
  if (['zip', 'tar', 'gz', 'tgz', '7z', 'rar', 'xz', 'zst', 'bz2'].includes(ext)) return 'file-archive';
  if (['js', 'ts', 'rs', 'py', 'go', 'c', 'h', 'cpp', 'java', 'json', 'toml', 'yaml', 'yml', 'html', 'css', 'sh', 'svelte'].includes(ext)) return 'file-code';
  if (mime.startsWith('text/') || ['md', 'txt', 'pdf', 'doc', 'docx', 'odt', 'rtf'].includes(ext)) return 'file-text';
  return 'file';
}

const collator = new Intl.Collator(undefined, { numeric: true, sensitivity: 'base' });

/**
 * Sort folder rows in place: folders first, then by `key` ('name', 'size'
 * or 'modified') in `dir` order, ties broken by name.
 */
export function sortEntries(rows, { key = 'name', dir = 'asc' } = {}) {
  const sign = dir === 'desc' ? -1 : 1;
  const value = {
    size: (r) => (r.node.kind === 'folder' ? 0 : r.meta.size),
    modified: changedAt,
  }[key];
  return rows.sort((a, b) => {
    if (a.node.kind !== b.node.kind) return a.node.kind === 'folder' ? -1 : 1;
    const byName = collator.compare(a.meta.name, b.meta.name);
    // Ties stay A to Z whichever way the sort goes.
    return value ? sign * (value(a) - value(b)) || byName : sign * byName;
  });
}

/** "1 file", "3 files". */
export function plural(n, word) {
  return `${n} ${word}${n === 1 ? '' : 's'}`;
}

/** Why a file or folder name isn't allowed, or '' if it's fine. */
export function nameError(n) {
  if (!n) return 'Enter a name.';
  if (n === '.' || n === '..' || n.includes('/')) return 'Names cannot contain "/" or be "." or "..".';
  if (n.length > 255) return 'That name is too long.';
  return '';
}
