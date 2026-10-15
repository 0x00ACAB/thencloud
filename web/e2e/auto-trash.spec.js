// A date for a file to go to the trash on its own: set from its menu, shown
// on its row, and taken back. The server learns the date, never the name.
import { test, expect } from '@playwright/test';
import { watchRequests, uniqueName, signUp, upload, rowMenu } from './helpers.js';

const PASSWORD = 'correct horse battery e2e';
const FILE = 'Boarding pass 71e0c3.pdf';

test('a file can be set to go to the trash on its own', async ({ page, context }) => {
  const requests = watchRequests(context);
  await signUp(page, uniqueName('tina'), PASSWORD);
  await upload(page, { [FILE]: '%PDF-1.4 not really' });

  await rowMenu(page, FILE, 'Delete automatically');
  const dialog = page.getByRole('dialog', { name: 'Delete automatically' });
  await expect(dialog).toContainText('the server is told the date; it never learns the name');
  await dialog.getByLabel('In a week').check();
  await dialog.getByRole('button', { name: 'Save' }).click();
  await expect(dialog).toHaveCount(0);

  const row = page.locator('tr', { hasText: FILE });
  await expect(row.locator('.badge', { hasText: 'In 7 days' })).toBeVisible();

  // Taken back, the badge goes.
  await rowMenu(page, FILE, 'Deleting automatically');
  await page.getByRole('dialog', { name: 'Delete automatically' }).getByRole('button', { name: "Don't delete it" }).click();
  await expect(row.locator('.badge', { hasText: 'In 7 days' })).toHaveCount(0);

  await requests.expectNone([FILE, 'Boarding pass', PASSWORD]);
});
