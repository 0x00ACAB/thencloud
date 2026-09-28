// Every string passed to t() has a Polish and a German translation with the
// same {placeholders}, plural forms for the language's plural categories,
// and no translation is left over for a string that's gone.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readdirSync, readFileSync, statSync } from 'node:fs';
import { join } from 'node:path';
import en from '../src/lib/messages/en.js';
import pl from '../src/lib/messages/pl.js';
import de from '../src/lib/messages/de.js';

function files(dir) {
  return readdirSync(dir).flatMap((f) => {
    const p = join(dir, f);
    if (statSync(p).isDirectory()) return f === 'wasm' || f === 'messages' ? [] : files(p);
    return /\.(svelte|js)$/.test(f) ? [p] : [];
  });
}

/** The literal first arguments of t(...) in the source. */
export function usedKeys() {
  const keys = new Set();
  const re = /\bt\(\s*('(?:\\.|[^'\\])*'|"(?:\\.|[^"\\])*"|`(?:\\.|[^`\\$])*`)/g;
  for (const f of files(new URL('../src', import.meta.url).pathname)) {
    for (const m of readFileSync(f, 'utf8').matchAll(re)) keys.add(Function(`return ${m[1]}`)());
  }
  return keys;
}

const placeholders = (s) => [...s.matchAll(/\{(\w+)\}/g)].map((m) => m[1]).sort().join(',');
const forms = (msg) => (typeof msg === 'object' ? Object.values(msg) : [msg]);

for (const [lang, catalog] of [['pl', pl], ['de', de]]) {
  test(`i18n: ${lang} has every string, and nothing extra`, () => {
    const used = usedKeys();
    const missing = [...used].filter((k) => !(k in catalog));
    assert.deepEqual(missing, [], `missing ${lang} translations`);
    const extra = Object.keys(catalog).filter((k) => !used.has(k));
    assert.deepEqual(extra, [], `${lang} translations no longer used`);
  });

  test(`i18n: ${lang} keeps the placeholders and has its plural forms`, () => {
    const categories = new Intl.PluralRules(lang).resolvedOptions().pluralCategories;
    for (const [key, msg] of Object.entries(catalog)) {
      // A plural form may leave out {count} ("this photo"), nothing else.
      const without = (x) => placeholders(x).split(',').filter((p) => p && (typeof msg !== 'object' || p !== 'count')).join(',');
      for (const f of forms(msg)) assert.equal(without(f), without(key), `${lang}: ${key}`);
      if (typeof msg === 'object') {
        assert.ok(key.includes('{count}'), `${lang}: plural forms need {count}: ${key}`);
        for (const c of categories) assert.ok(c in msg || 'other' in msg, `${lang}: ${key} lacks ${c}`);
        if (lang === 'pl') for (const c of ['one', 'few', 'many']) assert.ok(c in msg, `pl: ${key} lacks ${c}`);
      }
    }
  });
}

test('i18n: English plural forms are for strings in use', () => {
  const used = usedKeys();
  for (const key of Object.keys(en)) assert.ok(used.has(key), `en: ${key} is not used`);
});
