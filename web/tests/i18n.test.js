// Every string passed to t() has a Polish and a German translation with the
// same {placeholders}, plural forms for the language's plural categories,
// and no translation is left over for a string that's gone.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readdirSync, readFileSync } from 'node:fs';
import { join } from 'node:path';
import en from '../src/lib/messages/en.js';
import pl from '../src/lib/messages/pl.js';
import de from '../src/lib/messages/de.js';

function files(dir) {
  return readdirSync(dir, { withFileTypes: true }).flatMap((entry) => {
    const f = entry.name;
    const p = join(dir, f);
    if (entry.isDirectory()) return f === 'wasm' || f === 'messages' ? [] : files(p);
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
      // A translation may leave placeholders out (a plural form "this
      // photo", Polish without {their}) but never bring in new ones.
      const keyHas = new Set(placeholders(key).split(','));
      for (const f of forms(msg)) {
        for (const p of placeholders(f).split(',').filter(Boolean)) assert.ok(keyHas.has(p), `${lang}: {${p}} isn't in ${key}`);
      }
      const gendered = typeof msg === 'object' && ['feminine', 'masculine', 'neuter'].some((g) => g in msg);
      if (gendered) {
        assert.ok('other' in msg, `${lang}: ${key} needs an "other" form for when no gender is said`);
        for (const k of Object.keys(msg)) assert.ok(['feminine', 'masculine', 'neuter', 'other'].includes(k), `${lang}: ${key} has an odd form ${k}`);
      } else if (typeof msg === 'object') {
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

test('i18n: no catalog lists a key twice', () => {
  // A second entry would silently replace the first.
  for (const lang of ['en', 'pl', 'de']) {
    const src = readFileSync(new URL(`../src/lib/messages/${lang}.js`, import.meta.url), 'utf8');
    const keys = [...src.matchAll(/^ {2}('(?:\\.|[^'\\])*'|"(?:\\.|[^"\\])*"|\w+):/gm)].map((m) => (/^\w/.test(m[1]) ? m[1] : Function(`return ${m[1]}`)()));
    const twice = keys.filter((k, i) => keys.indexOf(k) !== i);
    assert.deepEqual(twice, [], `${lang} lists these twice`);
  }
});

test('i18n: nothing names a local variable t', () => {
  // It would hide the t() in scope (and the key scan can't tell).
  const local = /\b(?:const|let|var)\s+t\b|\bas\s+t\b|\(\s*t\s*(?:,[^)]*)?\)\s*=>|\bfor\s*\(\s*(?:const|let)\s+t\s+of\b/;
  const found = [];
  const walk = (dir) => {
    for (const entry of readdirSync(dir, { withFileTypes: true })) {
      const f = entry.name;
      const p = join(dir, f);
      if (entry.isDirectory()) {
        if (f !== 'wasm' && f !== 'messages') walk(p);
      } else if (/\.(svelte|js)$/.test(f)) {
        const src = readFileSync(p, 'utf8');
        if (!/\bt\(\s*['"`]/.test(src)) continue;
        src.split('\n').forEach((line, i) => local.test(line) && found.push(`${p}:${i + 1}: ${line.trim()}`));
      }
    }
  };
  walk(new URL('../src', import.meta.url).pathname);
  assert.deepEqual(found, []);
});
