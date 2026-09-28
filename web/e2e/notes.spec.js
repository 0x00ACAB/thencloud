// Notes: a folder of Markdown files as a notebook, saved as you type.
import { test, expect } from '@playwright/test';
import { watchRequests, uniqueName, signUp } from './helpers.js';

const NOTE = 'Remember the blue umbrella 5c2e';

test('notes are written, found and kept without the server reading them', async ({ page, context }) => {
  const requests = watchRequests(context);
  await signUp(page, uniqueName('noter'), 'notes password e2e');

  await page.getByRole('button', { name: 'New folder' }).click();
  await page.locator('dialog[open] input').fill('Journal 5c2e');
  await page.locator('dialog[open]').getByRole('button', { name: 'Create' }).click();

  await page.getByRole('button', { name: 'Notes', exact: true }).click();
  await page.getByRole('button', { name: 'Choose a folder' }).click();
  await page.locator('dialog[open]').getByRole('button', { name: 'Journal 5c2e' }).click();
  await page.locator('dialog[open]').getByRole('button', { name: 'Use Journal 5c2e' }).click();

  await page.getByRole('button', { name: 'New note' }).click();
  const editor = page.locator('main [contenteditable=true]');
  await editor.click();
  await page.keyboard.type(NOTE);
  await expect(page.getByRole('status').filter({ hasText: 'Saved' })).toBeVisible();

  // A second note, then search finds the first by what it says.
  await page.getByRole('button', { name: 'New note' }).click();
  await expect(page.locator('main aside li')).toHaveCount(2);
  await page.getByPlaceholder('Search notes').fill('umbrella');
  await expect(page.locator('main aside li')).toHaveCount(1);
  await expect(page.locator('main aside li')).toContainText(NOTE);

  // Reopened after a reload, it's all still there.
  await page.getByPlaceholder('Search notes').fill('');
  await page.reload();
  await page.getByLabel('Username').waitFor();
  expect(page.url()).toMatch(/#\/notes$/);

  await requests.expectNone([NOTE, 'umbrella', 'Journal 5c2e', 'notes password e2e']);
});
