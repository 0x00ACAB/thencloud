// Language and region: how dates, times, numbers and units are shown. Each
// setting can follow the browser ("auto"). Kept in localStorage so pages
// show the right format before sign-in (it's no secret), and in the
// encrypted "prefs" app data so it follows the account to other devices.
// The interface itself is English for now; `language` is where a language
// switcher will go.

const KEY = 'format';

/** Formats offered besides the browser's own, named in their own language. */
export const REGIONS = [
  ['en-US', 'English (United States)'],
  ['en-GB', 'English (United Kingdom)'],
  ['en-AU', 'English (Australia)'],
  ['en-CA', 'English (Canada)'],
  ['de-DE', 'Deutsch (Deutschland)'],
  ['fr-FR', 'Français (France)'],
  ['es-ES', 'Español (España)'],
  ['it-IT', 'Italiano (Italia)'],
  ['nl-NL', 'Nederlands (Nederland)'],
  ['pl-PL', 'Polski (Polska)'],
  ['pt-BR', 'Português (Brasil)'],
  ['sv-SE', 'Svenska (Sverige)'],
  ['uk-UA', 'Українська (Україна)'],
  ['ja-JP', '日本語 (日本)'],
  ['ko-KR', '한국어 (대한민국)'],
  ['zh-CN', '中文 (中国)'],
];

const DEFAULTS = { language: 'en', region: 'auto', clock: 'auto', units: 'auto' };

function stored() {
  try {
    const v = JSON.parse(localStorage.getItem(KEY) ?? '{}');
    return {
      language: 'en',
      region: REGIONS.some(([r]) => r === v.region) ? v.region : 'auto',
      clock: ['12', '24'].includes(v.clock) ? v.clock : 'auto',
      units: ['metric', 'imperial'].includes(v.units) ? v.units : 'auto',
    };
  } catch {
    return { ...DEFAULTS };
  }
}

/** { language, region: 'auto' | tag, clock: 'auto' | '12' | '24', units: 'auto' | 'metric' | 'imperial' } */
export const format = $state(stored());

/** The locale tag dates and numbers are formatted in. */
export const regionTag = () => (format.region === 'auto' ? navigator.language || 'en-US' : format.region);

/** Countries that measure in pounds and inches day to day. */
const IMPERIAL = new Set(['US', 'LR', 'MM']);

/** metric or imperial, after "auto" is resolved from the region. */
export function unitSystem() {
  if (format.units !== 'auto') return format.units;
  try {
    return IMPERIAL.has(new Intl.Locale(regionTag()).maximize().region) ? 'imperial' : 'metric';
  } catch {
    return 'metric';
  }
}

/** Change some settings, here and (when signed in) in the account. */
export function setFormat(change, save) {
  Object.assign(format, change);
  try {
    localStorage.setItem(KEY, JSON.stringify(format));
  } catch {
    /* this device just won't remember it */
  }
  return save?.((d) => (d.format = { ...format }));
}

/** Take the account's settings (from the "prefs" app data) after sign-in. */
export function applyAccountFormat(saved) {
  if (!saved) return;
  const next = { ...format, ...saved };
  if (JSON.stringify(next) !== JSON.stringify(format)) setFormat(next);
}

// Formatters are costly to make; keep one per settings and options.
const cache = new Map();
function formatter(kind, opts) {
  const tag = regionTag();
  const key = `${kind}|${tag}|${format.clock}|${JSON.stringify(opts)}`;
  let f = cache.get(key);
  if (!f) {
    if (kind === 'rel') f = new Intl.RelativeTimeFormat(tag, opts);
    else if (kind === 'num') f = new Intl.NumberFormat(tag, opts);
    else {
      const withTime = opts.timeStyle || opts.hour;
      f = new Intl.DateTimeFormat(tag, withTime && format.clock !== 'auto' ? { ...opts, hourCycle: format.clock === '12' ? 'h12' : 'h23' } : opts);
    }
    cache.set(key, f);
  }
  return f;
}

/** A date (and time, if the options ask for one) in the chosen format. */
export const formatDateTime = (ms, opts = { dateStyle: 'medium', timeStyle: 'short' }) => formatter('date', opts).format(ms);
export const formatNumber = (n, opts = {}) => formatter('num', opts).format(n);
export const relativeTime = (value, unit) => formatter('rel', { numeric: 'auto' }).format(value, unit);
