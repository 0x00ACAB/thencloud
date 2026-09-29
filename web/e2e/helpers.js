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
      headers: r.allHeaders().then(JSON.stringify, () => JSON.stringify(r.headers())),
      body: r.postDataBuffer() ?? Buffer.alloc(0),
    }),
  );
  return {
    async expectNone(secrets) {
      const needles = secrets.filter(Boolean).map((s) => Buffer.from(s));
      expect(seen.length).toBeGreaterThan(0);
      const own = new URL(test.info().project.use.baseURL).origin;
      for (const r of seen) {
        const { protocol, origin } = new URL(r.raw);
        expect(['blob:', 'data:'].includes(protocol) || origin === own, `${r.what} goes somewhere other than ${own}`).toBe(true);
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

export async function signUp(page, username, password) {
  await page.goto('/');
  await page.getByRole('tab', { name: 'Create account' }).click();
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
