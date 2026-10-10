import { readFileSync } from 'node:fs';
import { expect, test } from '@playwright/test';

/**
 * Record every request a browser context makes, so a test can check none
 * of them carried a secret: a file's name or contents, a password, a key.
 * Requests the service worker answers itself (/_stream/) never reach the
 * server, but they're checked too. Every request must also go to the
 * server under test (or stay in the browser, as blob: and data: URLs do):
 * the page talks to nothing else, no analytics, fonts or CDNs.
 */
export function watchRequests(context) {
  const seen = [];
  // Taken as each request is made: the context may be closed before the check.
  context.on('request', (r) =>
    seen.push({
      what: `${r.method()} ${r.url()}`,
      raw: r.url(),
      url: decodeURIComponent(r.url()),
      // allHeaders() can stay pending for good (a stream that never ends, a
      // closed context); then the headers known when it was sent will do.
      headers: Promise.race([r.allHeaders(), new Promise((resolve) => setTimeout(resolve, 5000).unref())]).then(
        (h) => JSON.stringify(h ?? r.headers()),
        () => JSON.stringify(r.headers()),
      ),
      body: r.postDataBuffer() ?? Buffer.alloc(0),
    }),
  );
  return {
    // `elsewhere`: other origins a test deliberately goes to (the stand-in
    // for Google's consent page), still checked for secrets.
    async expectNone(secrets, { elsewhere = [] } = {}) {
      const needles = secrets.filter(Boolean).map((s) => Buffer.from(s));
      expect(seen.length).toBeGreaterThan(0);
      const own = new URL(test.info().project.use.baseURL).origin;
      for (const r of seen) {
        const { protocol, origin } = new URL(r.raw);
        const allowed = ['blob:', 'data:'].includes(protocol) || origin === own || elsewhere.includes(origin);
        expect(allowed, `${r.what} goes somewhere other than ${own}`).toBe(true);
        const headers = await r.headers;
        for (const n of needles) {
          const what = `${r.what} carries ${JSON.stringify(n.toString())}`;
          expect(r.url.includes(n.toString()), what).toBe(false);
          expect(headers.includes(n.toString()), what).toBe(false);
          expect(r.body.includes(n), what).toBe(false);
        }
      }
    },
  };
}

let counter = 0;
/** A username unique to this run. */
export const uniqueName = (base) => `${base}${Date.now().toString(36)}${counter++}`;

/**
 * The setup code the first account on a new server needs: from the data
 * directory of the server playwright.config.js starts, or THENCLOUD_E2E_SETUP_CODE.
 */
function setupCode() {
  if (process.env.THENCLOUD_E2E_SETUP_CODE) return process.env.THENCLOUD_E2E_SETUP_CODE;
  return readFileSync('../target/e2e-data/setup-code', 'utf8').trim();
}

export async function signUp(page, username, password) {
  const options = page.waitForResponse((r) => r.url().includes('/api/auth/options'));
  await page.goto('/');
  const setup = (await (await options).json()).setup;
  await page.getByRole('tab', { name: 'Create account' }).click();
  if (setup) await page.getByLabel('Setup code').fill(setupCode());
  await expect(page.getByText('There is no password reset.')).toBeVisible();
  await page.getByLabel('Username').fill(username);
  await page.getByLabel('Password', { exact: true }).fill(password);
  await page.getByLabel('Confirm password').fill(password);
  await page.getByRole('button', { name: 'Create account' }).last().click();
  await expect(page.getByRole('heading', { name: 'My files' })).toBeVisible({ timeout: 60_000 });
}

/** Upload files (name -> contents) into the folder being shown. */
export async function upload(page, files) {
  await page.locator('input[type=file][multiple]').setInputFiles(
    Object.entries(files).map(([name, content]) => ({ name, mimeType: 'application/octet-stream', buffer: Buffer.from(content) })),
  );
  for (const name of Object.keys(files)) await expect(page.locator('button.row-open', { hasText: name })).toBeVisible();
}

export async function rowMenu(page, name, item) {
  await page.getByRole('button', { name: `Actions for ${name}` }).click();
  await page.locator('.menu-item').filter({ hasText: new RegExp(`^\\s*${item}\\s*$`) }).click();
}
