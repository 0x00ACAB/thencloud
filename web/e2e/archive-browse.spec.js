// Looking inside a .zip, .tar.gz or .7z in the preview (components/preview/ArchiveView.svelte):
// folders are navigable and a single entry downloads decrypted, without the
// whole archive ever reaching the server as anything but ciphertext.
import { test, expect } from '@playwright/test';
import { watchRequests, uniqueName, signUp, upload } from './helpers.js';
import { zip } from '../tests/zipfile.js';
import { tar } from '../tests/tarfile.js';
import { gzipSync } from 'node:zlib';
import { readFileSync } from 'node:fs';
import { FILES } from '../tests/sevenzfiles.js';

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

test('a .tar.gz can be browsed and an entry downloads decrypted', async ({ page, context }) => {
  const requests = watchRequests(context);
  await signUp(page, uniqueName('tara'), PASSWORD);
  const name = 'Backup 7c2d10.tar.gz';
  const bytes = gzipSync(tar({ 'top-7c2d10.txt': ROOT_SECRET, 'inner/deep-7c2d10.txt': NESTED_SECRET }));
  await upload(page, { [name]: bytes });

  await page.locator('button.row-open', { hasText: name }).click();
  const preview = page.locator('dialog.preview');
  await expect(preview.getByText('top-7c2d10.txt')).toBeVisible();
  await preview.getByText('inner', { exact: true }).click();
  await expect(preview.getByText('deep-7c2d10.txt')).toBeVisible();

  const download = page.waitForEvent('download');
  await preview.getByRole('button', { name: 'Download deep-7c2d10.txt' }).click();
  const saved = await download;
  const chunks = [];
  for await (const c of await saved.createReadStream()) chunks.push(c);
  expect(Buffer.concat(chunks).toString()).toBe(NESTED_SECRET);

  await requests.expectNone([ROOT_SECRET, NESTED_SECRET, name, PASSWORD]);
});

test('a .7z can be browsed and an entry downloads decrypted', async ({ page, context }) => {
  const requests = watchRequests(context);
  await signUp(page, uniqueName('seven'), PASSWORD);
  const name = 'Old projects 5a1c08.7z';
  // Solid, with BCJ2 in front of LZMA2: what 7-Zip makes for programs at its highest setting.
  const bytes = readFileSync(new URL('../tests/fixtures/7z/bcj2.7z', import.meta.url));
  const locked = 'Locked 5a1c08.7z';
  await upload(page, { [name]: bytes, [locked]: readFileSync(new URL('../tests/fixtures/7z/password.7z', import.meta.url)) });

  await page.locator('button.row-open', { hasText: name }).click();
  const preview = page.locator('dialog.preview');
  await expect(preview.getByText('a.txt')).toBeVisible();
  await preview.getByText('dir', { exact: true }).click();
  await expect(preview.getByText('b.txt')).toBeVisible();
  await expect(preview.getByText('deeper', { exact: true })).toBeVisible();

  const download = page.waitForEvent('download');
  await preview.getByRole('button', { name: 'Download b.txt' }).click();
  const saved = await download;
  const chunks = [];
  for await (const c of await saved.createReadStream()) chunks.push(c);
  expect(Buffer.concat(chunks)).toEqual(Buffer.from(FILES['dir/b.txt']));
  await page.keyboard.press('Escape');

  // A password-protected one still lists its files, and says why one can't be read.
  await page.locator('button.row-open', { hasText: locked }).click();
  await expect(preview.getByText('a.txt')).toBeVisible();
  await preview.getByRole('button', { name: 'Download a.txt' }).click();
  await expect(page.getByText("This archive is protected with a password, so it can't be opened here.")).toBeVisible();

  await requests.expectNone(['All work and no play', name, locked, PASSWORD]);
});
