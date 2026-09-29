// Lists strings passed to t() that a catalog lacks, and translations no
// longer used:  node scripts/i18n-missing.mjs [pl|de]
import { readdirSync, readFileSync, statSync } from 'node:fs';
import { join } from 'node:path';

function files(dir) {
  return readdirSync(dir).flatMap((f) => {
    const p = join(dir, f);
    if (statSync(p).isDirectory()) return f === 'wasm' || f === 'messages' ? [] : files(p);
    return /\.(svelte|js)$/.test(f) ? [p] : [];
  });
}

const used = new Set();
const re = /\bt\(\s*('(?:\\.|[^'\\])*'|"(?:\\.|[^"\\])*"|`(?:\\.|[^`\\$])*`)/g;
for (const f of files('src')) for (const m of readFileSync(f, 'utf8').matchAll(re)) used.add(Function(`return ${m[1]}`)());

for (const lang of process.argv.slice(2).length ? process.argv.slice(2) : ['pl', 'de']) {
  const catalog = (await import(`../src/lib/messages/${lang}.js`)).default;
  const missing = [...used].filter((k) => !(k in catalog));
  const extra = Object.keys(catalog).filter((k) => !used.has(k));
  console.log(`${lang}: ${missing.length} missing, ${extra.length} unused`);
  for (const k of missing) console.log(`  ${JSON.stringify(k)}`);
  for (const k of extra) console.log(`  unused: ${JSON.stringify(k)}`);
}
