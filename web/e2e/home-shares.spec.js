// A folder shared with you can be shown in My files ("Show in My files" in
// Shared with me). Which ones is kept in the encrypted app data, so the
// server never learns it, nor the folder's name.
import { test, expect } from '@playwright/test';
import { watchRequests, uniqueName, signUp, upload, rowMenu } from './helpers.js';

const PASSWORD = 'correct horse battery e2e';
const FOLDER = 'Uni notes 3d9a71';
const FILE = 'Lecture 3d9a71.md';

test('a folder shared with you can be shown in My files', async ({ page, context, browser }) => {
  const requests = watchRequests(context);
  const bobName = uniqueName('bob');
  const bobContext = await browser.newContext({ colorScheme: 'dark' });
  const bobRequests = watchRequests(bobContext);
  const bob = await bobContext.newPage();
  await signUp(bob, bobName, PASSWORD);

  await signUp(page, uniqueName('dana'), PASSWORD);
  await page.getByRole('button', { name: 'New folder' }).click();
  await page.locator('dialog[open] input').fill(FOLDER);
  await page.locator('dialog[open]').getByRole('button', { name: 'Create' }).click();
  await page.locator('button.row-open', { hasText: FOLDER }).click();
  await expect(page.getByRole('heading', { name: FOLDER })).toBeVisible();
  await upload(page, { [FILE]: '# Week one' });
  await page.getByRole('button', { name: 'My files' }).first().click();
  await rowMenu(page, FOLDER, 'Share');
  const dialog = page.locator('dialog[open]');
  await dialog.getByLabel('Username').fill(bobName);
  await dialog.getByRole('button', { name: 'Continue' }).click();
  await dialog.getByLabel('The fingerprints match').check();
  await dialog.getByRole('button', { name: 'Share', exact: true }).click();
  await expect(page.getByText(`Shared with ${bobName}`)).toBeVisible();

  await bob.getByRole('button', { name: 'Shared with me' }).first().click();
  await bob.getByRole('button', { name: 'Actions' }).click();
  await bob.getByRole('menuitem', { name: 'Show in My files' }).click();
  await expect(bob.getByText(`${FOLDER} now shows in My files`)).toBeVisible();

  await bob.getByRole('button', { name: 'My files' }).first().click();
  const shared = bob.getByRole('region', { name: 'Shared with you' });
  await expect(shared.getByRole('button', { name: FOLDER, exact: true })).toBeVisible();
  await shared.getByRole('button', { name: FOLDER, exact: true }).click();
  await expect(bob.locator('button.row-open', { hasText: FILE })).toBeVisible();

  // And taken away again, from My files.
  await bob.getByRole('button', { name: 'My files' }).first().click();
  await shared.getByRole('button', { name: `Actions for ${FOLDER}` }).click();
  await bob.getByRole('menuitem', { name: 'Remove from My files' }).click();
  await expect(shared).toHaveCount(0);
  await bobContext.close();

  for (const r of [requests, bobRequests]) await r.expectNone([FOLDER, FILE, '# Week one', PASSWORD]);
});
