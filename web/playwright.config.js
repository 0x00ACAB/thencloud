// Browser tests: the built web client against a real server.
//
//   npx playwright test            # starts a server with an empty data dir
//
// Build first (../build.sh). Without a browser of your own, run it in the
// Playwright image; see e2e/README.md.
import { defineConfig, devices } from '@playwright/test';

const port = process.env.THENCLOUD_E2E_PORT ?? '8093';
const server = process.env.THENCLOUD_SERVER ?? '../target/debug/thencloud-server';

export default defineConfig({
  testDir: 'e2e',
  timeout: 90_000,
  expect: { timeout: 15_000 },
  fullyParallel: false,
  workers: 1,
  retries: process.env.CI ? 1 : 0,
  reporter: process.env.CI ? [['list'], ['html', { open: 'never' }]] : 'list',
  use: {
    baseURL: `http://127.0.0.1:${port}`,
    colorScheme: 'dark',
    trace: 'retain-on-failure',
    ...devices['Desktop Chrome'],
  },
  webServer: {
    command: `rm -rf ../target/e2e-data && ${server} --bind 127.0.0.1:${port} --data-dir ../target/e2e-data --web-dir dist`,
    url: `http://127.0.0.1:${port}/api/health`,
    reuseExistingServer: true,
    timeout: 60_000,
  },
});
