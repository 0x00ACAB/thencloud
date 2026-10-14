// Tags and their colours live in the encrypted app data: a file can be
// tagged, the tag given a colour from the Tags view, and the colour shows
// on the file's row, without the tag or the name ever reaching the server.
import { test, expect } from '@playwright/test';
import { watchRequests, uniqueName, signUp, upload } from './helpers.js';

const PASSWORD = 'correct horse battery e2e';
const FILE = 'Invoice 5b81c4.txt';
const TAG = 'taxes-5b81c4';

test('a tag can be given a colour, shown on the rows it is on', async ({ page, context }) => {
  const requests = watchRequests(context);
  await signUp(page, uniqueName('tess'), PASSWORD);
  await upload(page, { [FILE]: 'amount due' });

  await page.getByRole('button', { name: `Actions for ${FILE}` }).first().click();
  await page.getByRole('menuitem', { name: 'Tags' }).click();
  const dialog = page.locator('dialog[open]');
  await dialog.getByLabel('Add a tag').fill(TAG);
  await dialog.getByLabel('Add a tag').press('Enter');
  await dialog.getByRole('button', { name: 'Save' }).click();
  await expect(dialog).toHaveCount(0);
  const row = page.locator('tr', { hasText: FILE });
  await expect(row.locator('.badge', { hasText: TAG })).toBeVisible();
  await expect(row.locator('.tag-dot')).toHaveCount(0);

  await page.locator('aside').getByRole('button', { name: 'Tags' }).click();
  await page.getByRole('radio', { name: 'Green' }).click();
  await expect(page.getByRole('radio', { name: 'Green' })).toHaveAttribute('aria-checked', 'true');
  await expect(page.getByRole('radio', { name: new RegExp(TAG) }).locator('.tag-dot')).toBeVisible();

  await page.locator('aside').getByRole('button', { name: 'My files' }).click();
  await expect(page.locator('tr', { hasText: FILE }).locator('.tag-dot')).toBeVisible();

  // Taking the colour away again.
  await page.locator('aside').getByRole('button', { name: 'Tags' }).click();
  await page.getByRole('radio', { name: 'No colour' }).click();
  await expect(page.getByRole('radio', { name: new RegExp(TAG) }).locator('.tag-dot')).toHaveCount(0);

  await requests.expectNone([FILE, TAG, 'green', PASSWORD]);
});
