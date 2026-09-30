// Which files the preview can show, and how.
//
// The MIME type comes from encrypted metadata that whoever uploaded the file
// chose, so for shared files it's untrusted. It only picks a viewer here; the
// decrypted bytes are always wrapped in a Blob with a type from the fixed
// lists below, never the stored one, so nothing can end up rendered as HTML.

const IMAGE = {
  png: 'image/png', jpg: 'image/jpeg', jpeg: 'image/jpeg', gif: 'image/gif', webp: 'image/webp',
  avif: 'image/avif', bmp: 'image/bmp', ico: 'image/x-icon', svg: 'image/svg+xml',
};
const VIDEO = { mp4: 'video/mp4', m4v: 'video/mp4', webm: 'video/webm', ogv: 'video/ogg', mov: 'video/quicktime' };
const AUDIO = {
  mp3: 'audio/mpeg', m4a: 'audio/mp4', m4b: 'audio/mp4', aac: 'audio/aac', ogg: 'audio/ogg', oga: 'audio/ogg', opus: 'audio/ogg',
  wav: 'audio/wav', flac: 'audio/flac', weba: 'audio/webm',
};
const MARKDOWN = new Set(['md', 'markdown', 'mdown', 'mkd']);
// Read by lib/office.js into plain text, tables and pictures.
const OFFICE = new Set(['docx', 'odt', 'xlsx', 'pptx']);

// Text files that don't have a text/* MIME type, or often have none at all.
const TEXT = new Set([
  'txt', 'log', 'csv', 'tsv', 'json', 'jsonc', 'json5', 'toml', 'yaml', 'yml', 'ini', 'cfg', 'conf', 'env', 'xml',
  'html', 'htm', 'css', 'scss', 'sass', 'less', 'js', 'mjs', 'cjs', 'jsx', 'ts', 'mts', 'cts', 'tsx', 'svelte', 'vue',
  'rs', 'go', 'py', 'rb', 'php', 'java', 'kt', 'kts', 'swift', 'c', 'h', 'cc', 'cpp', 'cxx', 'hpp', 'cs', 'lua',
  'sh', 'bash', 'zsh', 'fish', 'ps1', 'sql', 'diff', 'patch', 'dockerfile', 'makefile', 'mk', 'cmake', 'gradle',
  'nix', 'zig', 'hs', 'ex', 'exs', 'erl', 'clj', 'scala', 'dart', 'r', 'pl', 'tex', 'bib', 'srt', 'vtt', 'gitignore',
  'editorconfig', 'lock', 'properties', 'proto', 'graphql', 'gql', 'tf', 'hcl',
]);
const TEXT_NAMES = new Set(['dockerfile', 'makefile', 'license', 'readme', 'changelog', 'authors', 'copying', 'justfile']);

/** Largest file decrypted for a preview; everything is held in memory. */
export const MAX_PREVIEW = 256 * 1024 * 1024;
/** Text beyond this is shown as a download instead. */
export const MAX_TEXT = 5 * 1024 * 1024;

export function extension(name) {
  const i = name.lastIndexOf('.');
  return i > 0 ? name.slice(i + 1).toLowerCase() : '';
}

/**
 * How to preview a file: { kind, type } where kind is image, video, audio,
 * pdf, book, office, markdown or text and type is the Blob type to use, or
 * null if there is no preview for it. A book has `format` (epub or cbz), an
 * office document too (docx, odt, xlsx or pptx). CSV and TSV are text with `table` set, so they can
 * be shown as a table and still edited as text.
 */
export function previewKind(meta) {
  const ext = extension(meta.name);
  const mime = (meta.mime || '').toLowerCase();
  if (IMAGE[ext]) return { kind: 'image', type: IMAGE[ext] };
  if (VIDEO[ext]) return { kind: 'video', type: VIDEO[ext] };
  if (AUDIO[ext]) return { kind: 'audio', type: AUDIO[ext] };
  if (ext === 'pdf' || mime === 'application/pdf') return { kind: 'pdf', type: 'application/pdf' };
  if (ext === 'epub' || ext === 'cbz') return { kind: 'book', type: 'application/octet-stream', format: ext };
  if (OFFICE.has(ext)) return { kind: 'office', type: 'application/octet-stream', format: ext };
  if (MARKDOWN.has(ext) || mime === 'text/markdown') return { kind: 'markdown', type: 'text/plain' };
  if (ext === 'csv' || ext === 'tsv') return { kind: 'text', type: 'text/plain', table: true };
  if (
    TEXT.has(ext) ||
    TEXT_NAMES.has(meta.name.toLowerCase()) ||
    mime.startsWith('text/') ||
    /^application\/(json|xml|javascript|x-sh|toml|yaml)/.test(mime)
  ) {
    return { kind: 'text', type: 'text/plain' };
  }
  return null;
}

/** Decode text, or null if it looks binary (NUL bytes near the start). */
export async function readText(blob) {
  const bytes = new Uint8Array(await blob.arrayBuffer());
  if (bytes.subarray(0, 8000).includes(0)) return null;
  return new TextDecoder('utf-8').decode(bytes);
}
