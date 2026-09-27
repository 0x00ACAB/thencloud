// Subtitles next to a video: `Film.srt`, `Film.en.vtt`, `Film.pt-BR.forced.srt`.
// They're decrypted like any file, turned into WebVTT if they're SRT, and
// given to the <video> as blob: URLs. The browser renders cues as text, so
// a hostile subtitle file can't inject markup.

const SUBTITLE = /\.(srt|vtt)$/i;
const MAX_SUBTITLE = 4 * 1024 * 1024;

export const isSubtitle = (name) => SUBTITLE.test(name);

// Tags that look like language codes but mean something else.
const NOT_LANGUAGES = new Set(['sdh', 'cc']);

const stem = (name) => name.replace(/\.[^.]+$/, '').toLowerCase();

function languageName(code) {
  try {
    const name = new Intl.DisplayNames([navigator.language, 'en'], { type: 'language' }).of(code);
    return name && name.toLowerCase() !== code.toLowerCase() ? name : null;
  } catch {
    return null;
  }
}

/**
 * The subtitle files among `files` (entries in the video's folder) that go
 * with `videoName`, as [{ entry, lang, label }], sorted by label.
 */
export function matchSubtitles(videoName, files) {
  const base = stem(videoName);
  const baseLength = videoName.replace(/\.[^.]+$/, '').length;
  const out = [];
  for (const entry of files) {
    const name = entry.meta.name;
    if (!isSubtitle(name) || entry.meta.size > MAX_SUBTITLE) continue;
    const s = stem(name);
    if (s !== base && !s.startsWith(`${base}.`)) continue;
    // What's between the video's name and the extension: "en", "en.forced", "sdh".
    const tags = name.replace(/\.[^.]+$/, '').slice(baseLength).split('.').filter(Boolean);
    const lang = tags.find((t) => !NOT_LANGUAGES.has(t.toLowerCase()) && /^[a-z]{2,3}(-[a-z0-9]{2,8})?$/i.test(t) && languageName(t)) ?? '';
    const rest = tags.filter((t) => t !== lang).map((t) => (NOT_LANGUAGES.has(t.toLowerCase()) ? t.toUpperCase() : t));
    const language = lang ? languageName(lang) : 'Subtitles';
    const label = rest.length ? `${language} (${rest.join(', ')})` : language;
    out.push({ entry, lang, label });
  }
  // Two files that end up with the same label get numbered.
  const seen = new Map();
  for (const t of out) {
    const n = (seen.get(t.label) ?? 0) + 1;
    seen.set(t.label, n);
    if (n > 1) t.label = `${t.label} ${n}`;
  }
  return out.sort((a, b) => a.label.localeCompare(b.label));
}

/** WebVTT text from SRT (or WebVTT, which is passed through). */
export function toVtt(text) {
  const t = text.replace(/^﻿/, '').replace(/\r\n?/g, '\n');
  if (/^WEBVTT/.test(t)) return t;
  // SRT: the same cues, with a comma before the milliseconds.
  const body = t.replace(/(\d{1,2}:\d{2}:\d{2}),(\d{3})/g, '$1.$2');
  return `WEBVTT\n\n${body}`;
}

/** The track the browser should start with: one in the reader's language, else none. */
export function preferredTrack(tracks) {
  const want = (navigator.language || '').toLowerCase().split('-')[0];
  return tracks.findIndex((t) => t.lang && t.lang.toLowerCase().split('-')[0] === want);
}

/**
 * Decrypt the matched files into [{ label, lang, url }]. `fetch(entry)`
 * resolves to { blob }. Call `release` on the result when done.
 */
export async function loadSubtitles(matched, fetch) {
  const tracks = [];
  for (const m of matched) {
    try {
      const { blob } = await fetch(m.entry, () => {});
      const vtt = toVtt(await blob.text());
      tracks.push({ label: m.label, lang: m.lang, url: URL.createObjectURL(new Blob([vtt], { type: 'text/vtt' })) });
    } catch {
      /* one bad file doesn't stop the others */
    }
  }
  return tracks;
}

export function release(tracks) {
  for (const t of tracks) URL.revokeObjectURL(t.url);
}
