// Small pieces of global UI state: toasts, the upload queue and the theme.

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
  const i = toasts.findIndex((t) => t.id === id);
  if (i >= 0) toasts.splice(i, 1);
}

export function errorMessage(e) {
  if (e instanceof ApiError) return e.message;
  return String(e?.message || e || 'Something went wrong');
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

export const theme = $state({ pref: readTheme() });

const media = matchMedia('(prefers-color-scheme: dark)');

function applyTheme() {
  const dark = theme.pref === 'dark' || (theme.pref === 'system' && media.matches);
  document.documentElement.classList.toggle('dark', dark);
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

export async function copyText(text, what = 'Copied to clipboard') {
  try {
    await navigator.clipboard.writeText(text);
    toast(what, { kind: 'success' });
  } catch {
    toast('Could not access the clipboard', { kind: 'error' });
  }
}
