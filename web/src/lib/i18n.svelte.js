// Translations. The English text is the key, and {placeholders} in it are
// filled from the second argument (params such as name or count). A
// translation may leave a placeholder out (Polish has no use for {their}),
// but never add one. A translation can also have forms by grammatical
// gender (`feminine`, `masculine`, `neuter`, and `other` for "not said"),
// picked by a `gender` param; see `pronouns` for someone's English ones. A message
// with a {count} can have plural forms, chosen by Intl.PluralRules for the
// language (Polish has one, few and many; English and German one and
// other); English plural forms are in messages/en.js. Anything without a
// translation shows in English. Catalogs other than English are loaded
// when chosen. web/tests/i18n.test.js checks every t() string has a Polish
// and a German translation with the same placeholders.

import { format, formatNumber } from './locale.svelte.js';
import en from './messages/en.js';

export const LANGUAGES = [
  ['en', 'English'],
  ['pl', 'Polski'],
  ['de', 'Deutsch'],
];

const loaders = {
  pl: () => import('./messages/pl.js'),
  de: () => import('./messages/de.js'),
};

const catalogs = $state({ en });

/** The language in use: the chosen one, or the browser's first we have. */
export function language() {
  if (format.language !== 'auto') return format.language;
  for (const l of navigator.languages ?? [navigator.language]) {
    const code = String(l).slice(0, 2).toLowerCase();
    if (LANGUAGES.some(([c]) => c === code)) return code;
  }
  return 'en';
}

/** Load a language's messages (once). Resolves when they're in. */
export async function loadLanguage(lang = language()) {
  if (catalogs[lang] || !loaders[lang]) return;
  try {
    catalogs[lang] = (await loaders[lang]()).default;
  } catch {
    /* stays English */
  }
}

export const GENDERS = ['feminine', 'masculine', 'neuter'];
const isGendered = (msg) => GENDERS.some((g) => g in msg);

/**
 * Someone's English pronouns from their person details, for {they}, {them}
 * and {their} ({They} and {Their} to start a sentence), with their gender
 * for gendered forms. Not said: they, them, their.
 */
export function pronouns(details = {}) {
  const cap = (w) => w.charAt(0).toUpperCase() + w.slice(1);
  const they = details.subject || 'they';
  const their = details.possessive || 'their';
  return {
    they,
    They: cap(they),
    them: details.object || 'them',
    their,
    Their: cap(their),
    gender: details.gender ?? null,
  };
}

const plurals = new Map();
function pluralRules(lang) {
  if (!plurals.has(lang)) plurals.set(lang, new Intl.PluralRules(lang));
  return plurals.get(lang);
}

/** `key` in the current language, with {placeholders} from `params`. */
export function t(key, params) {
  const lang = language();
  let msg = catalogs[lang]?.[key] ?? en[key] ?? key;
  if (typeof msg === 'object') {
    msg = isGendered(msg) ? (msg[params?.gender] ?? msg.other) : (msg[pluralRules(lang).select(params?.count ?? 0)] ?? msg.other);
  }
  if (!params) return msg;
  // A {count} is shown the local way (1 234, 1.234 or 1,234).
  const shown = (name) => (name === 'count' && typeof params.count === 'number' ? formatNumber(params.count) : String(params[name]));
  return msg.replace(/\{(\w+)\}/g, (m, name) => (name in params ? shown(name) : m));
}

/**
 * A translated message split around the placeholders `t` left unfilled, for
 * putting components in them: strings for text, `{ slot: 'name' }` for each
 * placeholder, in the translation's word order.
 */
export function slots(text) {
  return text.split(/\{(\w+)\}/).map((s, i) => (i % 2 ? { slot: s } : s));
}
