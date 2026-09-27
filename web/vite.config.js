import { defineConfig } from 'vite';
import { svelte } from '@sveltejs/vite-plugin-svelte';
import tailwindcss from '@tailwindcss/vite';

// The Rust server serves `dist/` and enforces a strict CSP (same-origin
// scripts only, no inline scripts/styles, no data: URIs). The build output
// must stay compatible with that.
export default defineConfig({
  plugins: [
    tailwindcss(),
    svelte(),
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
    proxy: { '/api': 'http://127.0.0.1:8080' },
  },
});
