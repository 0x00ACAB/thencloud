import { defineConfig, minifySync } from 'vite';
import { svelte } from '@sveltejs/vite-plugin-svelte';
import tailwindcss from '@tailwindcss/vite';
import { readdirSync, readFileSync, writeFileSync, statSync } from 'node:fs';
import zlib from 'node:zlib';
import { promisify } from 'node:util';
import { optimize } from 'svgo';
import { join } from 'node:path';
import fileIcons, { SVGO } from './scripts/file-icons-plugin.mjs';

const gzip = promisify(zlib.gzip);
const brotli = promisify(zlib.brotliCompress);

// pdf.js loads fonts, CMaps and image decoders at runtime from these folders.
// They are copied to /pdfjs/ so the CSP's same-origin rule covers them.
// quickjs is only for running JavaScript inside PDFs, which we never do, and
// the _nowasm_fallback decoders only for browsers without WebAssembly, which
// can't run our crypto anyway.
const PDFJS = 'node_modules/pdfjs-dist';
const pdfjsAssets = () =>
  ['cmaps', 'standard_fonts', 'wasm', 'iccs'].flatMap((dir) =>
    readdirSync(join(PDFJS, dir))
      .filter((f) => !f.startsWith('quickjs') && !f.includes('_nowasm_fallback'))
      .map((f) => `${dir}/${f}`),
  );

// The Rust server serves `dist/` and enforces a strict CSP (same-origin
// scripts only, no inline scripts/styles, no data: URIs). The build output
// must stay compatible with that.
export default defineConfig({
  plugins: [
    tailwindcss(),
    svelte(),
    fileIcons(),
    {
      // Files in public/ are copied as they are: minify the scripts and the
      // favicon. Then write foo.js.gz and foo.js.br next to every
      // compressible file; the server sends the smallest one the browser
      // accepts (ffmpeg's 32 MB core becomes 10 gzipped, 8 with brotli).
      name: 'thencloud-optimise',
      apply: 'build',
      async closeBundle() {
        for (const f of ['sw.js', 'theme-init.js']) {
          const path = join('dist', f);
          writeFileSync(path, minifySync(f, readFileSync(path, 'utf8'), { module: false }).code);
        }
        writeFileSync('dist/favicon.svg', optimize(readFileSync('dist/favicon.svg', 'utf8'), SVGO).data);

        const walk = (dir) =>
          readdirSync(dir, { withFileTypes: true }).flatMap((e) => (e.isDirectory() ? walk(join(dir, e.name)) : [join(dir, e.name)]));
        const files = walk('dist').filter((f) => /\.(js|mjs|css|html|wasm|svg|json|bcmap|ttf|pfb|icc)$/.test(f) && statSync(f).size >= 1024);
        await Promise.all(
          files.map(async (f) => {
            const raw = readFileSync(f);
            const keep = (ext, out) => out.length < raw.length * 0.9 && writeFileSync(`${f}.${ext}`, out);
            keep('gz', await gzip(raw, { level: 9 }));
            keep(
              'br',
              await brotli(raw, {
                params: {
                  [zlib.constants.BROTLI_PARAM_SIZE_HINT]: raw.length,
                  // Level 11 on the 32 MB ffmpeg core takes minutes.
                  [zlib.constants.BROTLI_PARAM_QUALITY]: raw.length > 4 << 20 ? 9 : 11,
                  [zlib.constants.BROTLI_PARAM_LGWIN]: 24,
                },
              }),
            );
          }),
        );
      },
    },
    {
      name: 'thencloud-pdfjs-assets',
      configureServer(server) {
        const files = new Set(pdfjsAssets());
        server.middlewares.use((req, res, next) => {
          const path = req.url?.startsWith('/pdfjs/') && req.url.slice(7).split('?')[0];
          if (!path || !files.has(path)) return next();
          res.end(readFileSync(join(PDFJS, path)));
        });
      },
      generateBundle() {
        for (const f of pdfjsAssets()) this.emitFile({ type: 'asset', fileName: `pdfjs/${f}`, source: readFileSync(join(PDFJS, f)) });
      },
    },
    {
      // Mirror the server's `/s/<token>` route in dev.
      name: 'thencloud-share-route',
      configureServer(server) {
        server.middlewares.use((req, _res, next) => {
          if (req.url?.startsWith('/s/')) req.url = '/share.html';
          next();
        });
      },
    },
  ],
  build: {
    outDir: 'dist',
    emptyOutDir: true,
    assetsInlineLimit: 0,
    modulePreload: { polyfill: false },
    rollupOptions: {
      input: { index: 'index.html', share: 'share.html' },
    },
  },
  worker: { format: 'es' },
  // ffmpeg.wasm starts its own worker with new URL(..., import.meta.url);
  // pre-bundling would move the file away from that URL in dev.
  optimizeDeps: { exclude: ['@ffmpeg/ffmpeg'] },
  server: {
    // THENCLOUD_API points the dev server at a server on another address.
    proxy: { '/api': process.env.THENCLOUD_API ?? 'http://127.0.0.1:8080' },
  },
});
