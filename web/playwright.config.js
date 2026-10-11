// Browser tests: the built web client against a real server.
//
//   npx playwright test            # starts a server with an empty data dir
//
// Build first (../build.sh). Without a browser of your own, run it in the
// Playwright image; see e2e/README.md. THENCLOUD_E2E_URL tests a server
// that's already running instead (with THENCLOUD_E2E_PROXY, through a
// proxy: socks5://127.0.0.1:9050 for an onion service).
import { defineConfig, devices } from '@playwright/test';

const port = process.env.THENCLOUD_E2E_PORT ?? '8093';
const server = process.env.THENCLOUD_SERVER ?? '../target/debug/thencloud-server';
const target = process.env.THENCLOUD_E2E_URL;
const proxy = process.env.THENCLOUD_E2E_PROXY;
// A stand-in for Google, for linking Google Drive (e2e/fake-google.mjs).
const googlePort = process.env.THENCLOUD_E2E_GOOGLE_PORT ?? '8094';
const google = `--google-client-id e2e --google-client-secret e2e --google-test-base http://127.0.0.1:${googlePort}`;

export default defineConfig({
  testDir: 'e2e',
  timeout: proxy ? 300_000 : 90_000,
  expect: { timeout: proxy ? 60_000 : 15_000 },
  fullyParallel: false,
  workers: 1,
  retries: process.env.CI ? 1 : 0,
  reporter: process.env.CI ? [['list'], ['html', { open: 'never' }]] : 'list',
  use: {
    baseURL: target ?? `http://127.0.0.1:${port}`,
    proxy: proxy ? { server: proxy } : undefined,
    colorScheme: 'dark',
    trace: 'retain-on-failure',
    ...devices['Desktop Chrome'],
  },
  webServer: target ? undefined : [
    {
      command: `node e2e/fake-google.mjs ${googlePort}`,
      url: `http://127.0.0.1:${googlePort}/_health`,
      reuseExistingServer: true,
    },
    {
      command: `rm -rf ../target/e2e-data && ${server} --bind 127.0.0.1:${port} --data-dir ../target/e2e-data --web-dir dist ${google}`,
      url: `http://127.0.0.1:${port}/api/health`,
      reuseExistingServer: true,
      timeout: 60_000,
    },
  ],
});
