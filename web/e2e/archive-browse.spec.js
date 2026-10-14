// Looking inside a .zip in the preview (components/preview/ArchiveView.svelte):
// folders are navigable and a single entry downloads decrypted, without the
// whole archive ever reaching the server as anything but ciphertext.
import { test, expect } from '@playwright/test';
import { watchRequests, uniqueName, signUp, upload } from './helpers.js';
import { zip } from '../tests/zipfile.js';

const PASSWORD = 'correct horse battery e2e';
const ROOT_SECRET = 'TOP-SECRET-ROOT-3e91fa';
const NESTED_SECRET = 'TOP-SECRET-NESTED-91fa3e';
const ARCHIVE = 'Papers 3e91fa.zip';

test('an archive can be browsed by folder and a single entry downloads decrypted', async ({ page, context }) => {
  const requests = watchRequests(context);
  await signUp(page, uniqueName('dora'), PASSWORD);

  const bytes = zip({ 'root-3e91fa.txt': ROOT_SECRET, 'notes/nested-91fa3e.txt': NESTED_SECRET });
  await upload(page, { [ARCHIVE]: bytes });

  await page.locator('button.row-open', { hasText: ARCHIVE }).click();
  const preview = page.locator('dialog.preview');
  await expect(preview.getByText('root-3e91fa.txt')).toBeVisible();
  await expect(preview.getByText('notes', { exact: true })).toBeVisible();

  await preview.getByText('notes', { exact: true }).click();
  await expect(preview.getByText('nested-91fa3e.txt')).toBeVisible();
  await expect(preview.getByText('root-3e91fa.txt')).toHaveCount(0);

  const download = page.waitForEvent('download');
  await preview.getByRole('button', { name: 'Download nested-91fa3e.txt' }).click();
  const saved = await download;
  expect(saved.suggestedFilename()).toBe('nested-91fa3e.txt');
  const chunks = [];
  for await (const c of await saved.createReadStream()) chunks.push(c);
  expect(Buffer.concat(chunks).toString()).toBe(NESTED_SECRET);

  await requests.expectNone([ROOT_SECRET, NESTED_SECRET, ARCHIVE, PASSWORD]);
});
