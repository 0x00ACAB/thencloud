// The release manifest: the SHA-256 of every file in dist/ that the server
// serves, so anyone can check a running server sends exactly this build
// (`thencloud verify-web`). The .gz and .br copies aren't listed; they
// decompress to the files that are.
//
//   node scripts/manifest.mjs [out.json]

import { createHash } from 'node:crypto';
import { readdirSync, readFileSync, writeFileSync } from 'node:fs';
import { join, relative, sep } from 'node:path';

const walk = (dir) => readdirSync(dir, { withFileTypes: true }).flatMap((e) => (e.isDirectory() ? walk(join(dir, e.name)) : [join(dir, e.name)]));

const files = {};
for (const f of walk('dist').sort()) {
  if (/\.(gz|br)$/.test(f)) continue;
  files[relative('dist', f).split(sep).join('/')] = createHash('sha256').update(readFileSync(f)).digest('hex');
}
const pkg = JSON.parse(readFileSync('package.json', 'utf8'));
const manifest = `${JSON.stringify({ name: 'thencloud-web', version: process.env.THENCLOUD_VERSION || pkg.version || 'dev', files }, null, 2)}\n`;

const out = process.argv[2];
if (out) writeFileSync(out, manifest);
else process.stdout.write(manifest);
