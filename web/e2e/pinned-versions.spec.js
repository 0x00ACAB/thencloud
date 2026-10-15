// Pinning a version: the server learns only that it's pinned; the name
// stays in the encrypted app data.
import { test, expect } from '@playwright/test';
import { watchRequests, uniqueName, signUp, upload, rowMenu } from './helpers.js';

const PASSWORD = 'correct horse battery e2e';
const FILE = 'Thesis 9a31be.txt';
const NAME = 'Handed in 9a31be';

test('a version can be pinned with a name only the owner sees', async ({ page, context }) => {
  const requests = watchRequests(context);
  await signUp(page, uniqueName('pina'), PASSWORD);
  await upload(page, { [FILE]: 'chapter one' });

  await rowMenu(page, FILE, 'Version history');
  const history = page.getByRole('dialog', { name: 'Version history' });
  await history.getByRole('button', { name: 'Pin this version' }).click();
  const pin = page.getByRole('dialog', { name: 'Pin this version' });
  await pin.getByLabel('Name (optional)').fill(NAME);
  await pin.getByRole('button', { name: 'Pin' }).click();

  await expect(history.getByText(NAME)).toBeVisible();
  await expect(history.getByText('Pinned', { exact: true })).toBeVisible();
  await expect(history.getByRole('button', { name: 'Unpin this version' })).toHaveAttribute('aria-pressed', 'true');

  // Reopened, it's still pinned and named (the flag from the server, the name from the app data).
  await history.getByRole('button', { name: 'Done' }).click();
  await rowMenu(page, FILE, 'Version history');
  await expect(history.getByText(NAME)).toBeVisible();

  // Unpinning drops the name too.
  await history.getByRole('button', { name: 'Unpin this version' }).click();
  await expect(history.getByText('Pinned', { exact: true })).toHaveCount(0);
  await expect(history.getByText(NAME)).toHaveCount(0);

  await requests.expectNone([FILE, NAME, 'chapter one', PASSWORD]);
});
