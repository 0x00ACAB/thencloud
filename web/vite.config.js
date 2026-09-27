import { defineConfig } from 'vite';
import { svelte } from '@sveltejs/vite-plugin-svelte';
import tailwindcss from '@tailwindcss/vite';
import { readdirSync, readFileSync, writeFileSync, statSync } from 'node:fs';
import { gzipSync } from 'node:zlib';
import { join } from 'node:path';
import fileIcons from './scripts/file-icons-plugin.mjs';

// pdf.js loads fonts, CMaps and image decoders at runtime from these folders.
// They are copied to /pdfjs/ so the CSP's same-origin rule covers them.
// quickjs is only for running JavaScript inside PDFs, which we never do.
const PDFJS = 'node_modules/pdfjs-dist';
const pdfjsAssets = () =>
  ['cmaps', 'standard_fonts', 'wasm', 'iccs'].flatMap((dir) =>
    readdirSync(join(PDFJS, dir))
      .filter((f) => !f.startsWith('quickjs'))
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
      // Write foo.js.gz next to every compressible file; the server sends
      // those to browsers that accept gzip (ffmpeg's 32 MB core becomes 10).
      name: 'thencloud-precompress',
      apply: 'build',
      closeBundle() {
        const walk = (dir) =>
          readdirSync(dir, { withFileTypes: true }).flatMap((e) => (e.isDirectory() ? walk(join(dir, e.name)) : [join(dir, e.name)]));
        for (const f of walk('dist')) {
          if (!/\.(js|mjs|css|html|wasm|svg|json|bcmap|ttf|pfb|icc)$/.test(f) || statSync(f).size < 1024) continue;
          const gz = gzipSync(readFileSync(f), { level: 9 });
          if (gz.length < statSync(f).size * 0.9) writeFileSync(`${f}.gz`, gz);
        }
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
