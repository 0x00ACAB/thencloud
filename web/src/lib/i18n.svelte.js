// Translations. The English text is the key, and {placeholders} in it are
// filled from the second argument (params such as name or count). A message
// with a {count} can have plural forms, chosen by Intl.PluralRules for the
// language (Polish has one, few and many; English and German one and
// other); English plural forms are in messages/en.js. Anything without a
// translation shows in English. Catalogs other than English are loaded
// when chosen. web/tests/i18n.test.js checks every t() string has a Polish
// and a German translation with the same placeholders.

import { format } from './locale.svelte.js';
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
    const form = pluralRules(lang).select(params?.count ?? 0);
    msg = msg[form] ?? msg.other;
  }
  if (!params) return msg;
  return msg.replace(/\{(\w+)\}/g, (m, name) => (name in params ? String(params[name]) : m));
}
