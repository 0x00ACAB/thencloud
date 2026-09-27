import { previewKind } from './preview.js';

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

const rtf = new Intl.RelativeTimeFormat(undefined, { numeric: 'auto' });
const dateFmt = new Intl.DateTimeFormat(undefined, { day: 'numeric', month: 'short', year: 'numeric' });

/** "just now", "5 minutes ago", "yesterday", then a plain date. */
export function formatWhen(ms) {
  if (!ms) return '';
  const s = Math.round((ms - Date.now()) / 1000);
  const abs = Math.abs(s);
  if (abs < 45) return 'just now';
  if (abs < 3600) return rtf.format(Math.round(s / 60), 'minute');
  if (abs < 86400) return rtf.format(Math.round(s / 3600), 'hour');
  if (abs < 7 * 86400) return rtf.format(Math.round(s / 86400), 'day');
  return dateFmt.format(ms);
}

export function formatDate(ms) {
  return ms ? dateFmt.format(ms) : '';
}

export function fullDate(ms) {
  return ms ? new Date(ms).toLocaleString() : '';
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

export function sortEntries(rows) {
  return rows.sort((a, b) =>
    a.node.kind === b.node.kind
      ? a.meta.name.localeCompare(b.meta.name, undefined, { numeric: true, sensitivity: 'base' })
      : a.node.kind === 'folder'
        ? -1
        : 1,
  );
}

/** "1 file", "3 files". */
export function plural(n, word) {
  return `${n} ${word}${n === 1 ? '' : 's'}`;
}
