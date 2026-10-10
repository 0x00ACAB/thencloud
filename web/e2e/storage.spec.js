// Linking Google Drive (a stand-in, e2e/fake-google.mjs) as extra space,
// then moving one file there and back from "Where it is kept". Only
// ciphertext may reach the Drive.
import { expect, test } from '@playwright/test';
import { rowMenu, signUp, uniqueName, upload, watchRequests } from './helpers.js';

const PASSWORD = 'correct horse battery staple 7f3a91';
const NAME = 'drive-plan-7f3a91.txt';
const SECRET = 'Kept in the drive 7f3a91';
const GOOGLE = `http://127.0.0.1:${process.env.THENCLOUD_E2E_GOOGLE_PORT ?? '8094'}`;

test.skip(!!process.env.THENCLOUD_E2E_URL, 'needs the stand-in Google that playwright.config.js starts');

test('a file moves to a linked Google Drive and back, as ciphertext only', async ({ page, context }) => {
  const requests = watchRequests(context);
  await signUp(page, uniqueName('drive'), PASSWORD);
  await upload(page, { [NAME]: `${SECRET}\n` });

  // Link it from Settings: the consent page opens in a popup, which the
  // stand-in sends straight back; the app hears about it and says so.
  await page.getByRole('button', { name: 'Settings', exact: true }).first().click();
  const popup = context.waitForEvent('page');
  await page.getByRole('button', { name: 'Add Google Drive as extra space' }).click();
  await (await popup).waitForEvent('close');
  await expect(page.getByText('Google Drive is linked')).toBeVisible();
  await expect(page.getByText('e2e@example.com')).toBeVisible();

  // Move the file there.
  await page.getByRole('button', { name: 'My files', exact: true }).first().click();
  await rowMenu(page, NAME, 'Where it is kept');
  const dialog = page.locator('dialog[open]');
  await expect(dialog.getByText('This server')).toBeVisible();
  await dialog.getByRole('button', { name: 'Move to Google Drive' }).click();
  await expect(dialog.getByRole('button', { name: 'Move to this server' })).toBeVisible();
  await expect(dialog.locator('li')).toHaveCount(1);
  await expect(dialog.locator('li')).toContainText('Google Drive');

  const stored = await (await fetch(`${GOOGLE}/_files`)).json();
  expect(stored.length).toBeGreaterThan(0);
  for (const b64 of stored) expect(Buffer.from(b64, 'base64').includes(SECRET)).toBe(false);

  // It still opens from there, and comes back.
  await dialog.getByRole('button', { name: 'Close' }).click();
  await page.locator('button.row-open', { hasText: NAME }).click();
  await expect(page.locator('dialog.preview').getByText(SECRET)).toBeVisible();
  await page.locator('dialog.preview').getByRole('button', { name: 'Close preview' }).click();
  await rowMenu(page, NAME, 'Where it is kept');
  await dialog.getByRole('button', { name: 'Move to this server' }).click();
  await expect(dialog.getByRole('button', { name: 'Move to Google Drive' })).toBeVisible();
  await expect(dialog.locator('li')).toContainText('This server');

  await requests.expectNone([SECRET, NAME, PASSWORD], { elsewhere: [GOOGLE] });
});
