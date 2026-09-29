// Small pieces of global UI state: toasts, the upload queue and the theme.

import { t } from './i18n.svelte.js';
import { ApiError } from './api.js';

export const toasts = $state([]);
let toastSeq = 0;

/**
 * `action`: optional { label, onclick } shown as a button (e.g. Undo).
 * `icon`: optional icon name, overriding the one for `kind`.
 */
export function toast(message, { kind = 'info', timeout = 4000, action = null, icon = null } = {}) {
  const id = ++toastSeq;
  toasts.push({ id, message, kind, action, icon });
  setTimeout(() => dismissToast(id), action ? Math.max(timeout, 7000) : timeout);
}

export function dismissToast(id) {
  const i = toasts.findIndex((x) => x.id === id);
  if (i >= 0) toasts.splice(i, 1);
}

/** The server's errors that have a code worth translating; others show its own text. */
function apiMessage(e) {
  switch (e.code) {
    case 'forbidden':
      return t('You do not have permission to do that.');
    case 'not_found':
      return t('That no longer exists, or you no longer have access to it.');
    case 'name_taken':
      return t("There's already something with that name here.");
    case 'quota_exceeded':
      return t('Not enough storage left.');
    case 'rate_limited':
      return t('Too many attempts. Wait a little and try again.');
    case 'transfer_limit':
      return /upload/.test(e.message)
        ? t("You've reached today's upload limit on this server. It starts again at midnight UTC.")
        : t("You've reached today's download limit on this server. It starts again at midnight UTC.");
    case 'internal':
      return t('Something went wrong on the server. Try again in a moment.');
    default:
      return e.status === 0 ? t('Could not reach the server. Check your connection.') : e.message;
  }
}

export function errorMessage(e) {
  if (e instanceof ApiError) return apiMessage(e);
  return String(e?.message || e || t('Something went wrong'));
}

export function toastError(e) {
  console.error(e);
  toast(errorMessage(e), { kind: 'error', timeout: 6000 });
}

// Uploads and downloads shown in the bottom-right tray.
// status: 'active' | 'done' | 'error'; kind: 'upload' | 'download'
export const transfers = $state([]);
let transferSeq = 0;

export function trackTransfer(kind, name, size) {
  transfers.push({ id: ++transferSeq, kind, name, size, progress: 0, status: 'active', error: null });
  return transfers[transfers.length - 1];
}

export function clearFinishedTransfers() {
  for (let i = transfers.length - 1; i >= 0; i--) {
    if (transfers[i].status !== 'active') transfers.splice(i, 1);
  }
}

// Theme: 'system' | 'light' | 'dark'. Only this preference is persisted.
function readTheme() {
  try {
    return localStorage.getItem('theme') || 'system';
  } catch {
    return 'system';
  }
}

const media = matchMedia('(prefers-color-scheme: dark)');
const isDark = (pref) => pref === 'dark' || (pref === 'system' && media.matches);

/** `pref` is the setting; `dark` is what's showing now. */
export const theme = $state({ pref: readTheme(), dark: false });
theme.dark = isDark(theme.pref);

function applyTheme() {
  theme.dark = isDark(theme.pref);
  document.documentElement.classList.toggle('dark', theme.dark);
}

media.addEventListener('change', applyTheme);

export function setTheme(pref) {
  theme.pref = pref;
  try {
    localStorage.setItem('theme', pref);
  } catch {
    /* private mode */
  }
  applyTheme();
}

// Accent colour: one hex value; the CSS derives every shade from it.
export const DEFAULT_ACCENT = '#3b47f9';
export const ACCENT_PRESETS = [
  ['#3b47f9', 'Blue'],
  ['#0f9f8f', 'Teal'],
  ['#22a559', 'Green'],
  ['#e5a000', 'Amber'],
  ['#f2651d', 'Orange'],
  ['#e5484d', 'Red'],
  ['#e03e8c', 'Pink'],
  ['#8b5cf6', 'Violet'],
  ['#64748b', 'Slate'],
];

function luminance(hex) {
  const [r, g, b] = [1, 3, 5].map((i) => {
    const c = parseInt(hex.slice(i, i + 2), 16) / 255;
    return c <= 0.03928 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4;
  });
  return 0.2126 * r + 0.7152 * g + 0.0722 * b;
}

/** Contrast ratio between two hex colours (WCAG). */
export function contrast(a, b) {
  const [x, y] = [luminance(a), luminance(b)].sort((p, q) => q - p);
  return (x + 0.05) / (y + 0.05);
}

/** Black or white, whichever reads better on the accent. */
export function accentForeground(hex) {
  return contrast(hex, '#ffffff') >= contrast(hex, '#000000') ? '#ffffff' : '#000000';
}

const validHex = (v) => typeof v === 'string' && /^#[0-9a-f]{6}$/i.test(v);

function readAccent() {
  try {
    const v = localStorage.getItem('accent');
    return validHex(v) ? v.toLowerCase() : DEFAULT_ACCENT;
  } catch {
    return DEFAULT_ACCENT;
  }
}

export const accent = $state({ value: readAccent() });

export function setAccent(hex) {
  if (!validHex(hex)) return;
  accent.value = hex.toLowerCase();
  const root = document.documentElement.style;
  root.setProperty('--accent-base', accent.value);
  root.setProperty('--accent-fg', accentForeground(accent.value));
  try {
    if (accent.value === DEFAULT_ACCENT) localStorage.removeItem('accent');
    else localStorage.setItem('accent', accent.value);
  } catch {
    /* private mode */
  }
}

// Tint the neutrals (backgrounds, borders, grey text) with the accent's hue,
// Material-style. The shades are derived in app.css.
function readTint() {
  try {
    return localStorage.getItem('tint') === '1';
  } catch {
    return false;
  }
}

export const tint = $state({ on: readTint() });

export function setTint(on) {
  tint.on = on;
  document.documentElement.classList.toggle('tinted', on);
  try {
    if (on) localStorage.setItem('tint', '1');
    else localStorage.removeItem('tint');
  } catch {
    /* private mode */
  }
}

export async function copyText(text, what = 'Copied to clipboard') {
  try {
    await navigator.clipboard.writeText(text);
    toast(what, { kind: 'success' });
  } catch {
    toast('Could not access the clipboard', { kind: 'error' });
  }
}

// Files as a list or a grid of thumbnails. A display preference only.
function readFileView() {
  try {
    return localStorage.getItem('fileView') === 'grid' ? 'grid' : 'list';
  } catch {
    return 'list';
  }
}

export const fileView = $state({ value: readFileView() });

export function setFileView(v) {
  fileView.value = v;
  try {
    if (v === 'grid') localStorage.setItem('fileView', 'grid');
    else localStorage.removeItem('fileView');
  } catch {
    /* private mode */
  }
}

// File icon pack (see lib/file-icons.svelte.js). A display preference only.
function readIconPack() {
  try {
    const v = localStorage.getItem('iconPack');
    // Packs that were removed (Seti, vscode-icons) fall back to the default.
    return ['minimal', 'material', 'symbols', 'documents'].includes(v) ? v : 'minimal';
  } catch {
    return 'minimal';
  }
}

export const iconPack = $state({ value: readIconPack() });

export function setIconPack(id) {
  iconPack.value = id;
  try {
    localStorage.setItem('iconPack', id);
  } catch {
    /* private mode */
  }
}

// Icons for folders by name ("src", "images"...), from the chosen pack.
function readFolderIcons() {
  try {
    return localStorage.getItem('folderIcons') === 'named';
  } catch {
    return false;
  }
}

export const folderIcons = $state({ named: readFolderIcons() });

export function setFolderIcons(named) {
  folderIcons.named = named;
  try {
    localStorage.setItem('folderIcons', named ? 'named' : 'plain');
  } catch {
    /* private mode */
  }
}

// What to do with location and camera details in photos being uploaded:
// ask when a photo has a location, always remove them, or keep them.
function readPhotoDetails() {
  try {
    const v = localStorage.getItem('photoDetails');
    return ['ask', 'remove', 'keep'].includes(v) ? v : 'ask';
  } catch {
    return 'ask';
  }
}

export const photoDetails = $state({ value: readPhotoDetails() });

export function setPhotoDetails(v) {
  photoDetails.value = v;
  try {
    localStorage.setItem('photoDetails', v);
  } catch {
    /* private mode */
  }
}

// Sort order of folder listings, remembered per device.
function readSort() {
  try {
    const v = JSON.parse(localStorage.getItem('sort') || 'null');
    if (['name', 'size', 'modified'].includes(v?.key) && ['asc', 'desc'].includes(v?.dir)) return v;
  } catch {
    /* fall through */
  }
  return { key: 'name', dir: 'asc' };
}

export const sort = $state(readSort());

/** Sort by `key`; picking the current key again flips the direction. */
export function sortBy(key) {
  if (sort.key === key) sort.dir = sort.dir === 'asc' ? 'desc' : 'asc';
  else {
    sort.key = key;
    // Newest and largest first is what people usually want.
    sort.dir = key === 'name' ? 'asc' : 'desc';
  }
  try {
    localStorage.setItem('sort', JSON.stringify(sort));
  } catch {
    /* private mode */
  }
}
