// Small pieces of global UI state: toasts, the upload queue and the theme.

import { ApiError } from './api.js';

export const toasts = $state([]);
let toastSeq = 0;

export function toast(message, { kind = 'info', timeout = 4000 } = {}) {
  const id = ++toastSeq;
  toasts.push({ id, message, kind });
  setTimeout(() => dismissToast(id), timeout);
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

export async function copyText(text, what = 'Copied to clipboard') {
  try {
    await navigator.clipboard.writeText(text);
    toast(what, { kind: 'success' });
  } catch {
    toast('Could not access the clipboard', { kind: 'error' });
  }
}
