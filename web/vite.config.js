import { defineConfig } from 'vite';
import { svelte } from '@sveltejs/vite-plugin-svelte';
import tailwindcss from '@tailwindcss/vite';
import { readdirSync, readFileSync } from 'node:fs';
import { join } from 'node:path';

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
  server: {
    // THENCLOUD_API points the dev server at a server on another address.
    proxy: { '/api': process.env.THENCLOUD_API ?? 'http://127.0.0.1:8080' },
  },
});
