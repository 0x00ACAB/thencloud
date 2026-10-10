// The main paths through the web client, checking along the way that no
// request ever carries a file's name or contents, a password or a key.
import { test, expect } from '@playwright/test';
import { watchRequests, uniqueName, signUp, upload, rowMenu } from './helpers.js';

const PASSWORD = 'correct horse battery e2e';
const SECRET = 'TOP-SECRET-PAYLOAD-7f3a91';
const NAME = 'Quarterly plan 7f3a91.txt';

test('sign up, upload, preview, folders and search stay in the browser', async ({ page, context }) => {
  const requests = watchRequests(context);
  await signUp(page, uniqueName('alice'), PASSWORD);

  await upload(page, {
    [NAME]: `${SECRET}\nline two\n`,
    // Inline SVG could make the browser fetch a path (feImage, <a> inside
    // SVG); the preview must not let it.
    'notes-7f3a91.md':
      '# Heading 7f3a91\n\nSome *text*.\n\n<svg width="10" height="10"><filter id="f"><feImage href="leak-7f3a91.png"/></filter>' +
      '<rect width="10" height="10" filter="url(#f)"/><a href="leak-7f3a91-link"><text>x</text></a></svg>\n',
    'table-7f3a91.csv': 'name,score\nAda,10\nBob,2\n',
  });

  // Text, Markdown and CSV previews decrypt in the browser. The folder is
  // sorted by name: the note, the plan, the table.
  await page.locator('button.row-open', { hasText: 'notes-7f3a91.md' }).click();
  const preview = page.locator('dialog.preview');
  await expect(preview.getByRole('heading', { name: 'Heading 7f3a91' })).toBeVisible();
  await preview.getByRole('button', { name: 'Next file' }).click();
  await expect(preview.getByText(SECRET)).toBeVisible();
  await preview.getByRole('button', { name: 'Next file' }).click();
  await expect(preview.locator('table')).toContainText('Ada');
  await preview.getByRole('button', { name: 'Close preview' }).click();

  // A folder, and a file moved into it.
  await page.getByRole('button', { name: 'New folder' }).click();
  await page.locator('dialog[open] input').fill('Private folder 7f3a91');
  await page.locator('dialog[open]').getByRole('button', { name: 'Create' }).click();
  await rowMenu(page, NAME, 'Move');
  await page.locator('dialog[open]').getByRole('button', { name: /Private folder 7f3a91/ }).click();
  await page.locator('dialog[open]').getByRole('button', { name: /^Move/ }).click();
  await expect(page.locator('button.row-open', { hasText: NAME })).toHaveCount(0);

  // Searching everywhere decrypts names here; the server only lists folders.
  await page.getByPlaceholder('Search').fill('quarterly');
  await page.getByRole('radio', { name: 'Everywhere' }).click();
  await expect(page.getByRole('button', { name: new RegExp(NAME) })).toBeVisible();

  // The reload keeps the folder, by id only.
  await page.getByPlaceholder('Search').fill('');
  await page.locator('button.row-open', { hasText: 'Private folder 7f3a91' }).click();
  await expect(page).toHaveURL(/#\/files\/[0-9a-f-]{36}$/);

  await requests.expectNone([SECRET, NAME, 'Quarterly', 'Private folder', 'Heading 7f3a91', 'Ada,10', 'leak-7f3a91', PASSWORD]);
});

test('a public link opens without an account, and its key stays after the #', async ({ page, context, browser }) => {
  const requests = watchRequests(context);
  await signUp(page, uniqueName('linker'), PASSWORD);
  await upload(page, { [NAME]: SECRET });

  await rowMenu(page, NAME, 'Public link');
  const dialog = page.locator('dialog[open]');
  await expect(dialog.getByText('after #')).toBeVisible();
  await dialog.getByRole('button', { name: 'Create link' }).click();
  const url = (await dialog.locator('.font-mono', { hasText: '/s/' }).first().textContent()).trim();
  const key = url.split('#')[1];
  expect(key.length).toBeGreaterThan(20);

  // A visitor with no account.
  const visitor = await browser.newContext({ colorScheme: 'dark' });
  const visitorRequests = watchRequests(visitor);
  const v = await visitor.newPage();
  await v.goto(url);
  await expect(v.getByRole('heading', { name: NAME })).toBeVisible();
  await v.getByRole('button', { name: 'Preview' }).click();
  await expect(v.locator('dialog.preview').getByText(SECRET)).toBeVisible();
  await visitor.close();

  for (const r of [requests, visitorRequests]) await r.expectNone([SECRET, NAME, PASSWORD, key]);
});

test('a link password is part of the key, and never leaves the browser', async ({ page, context, browser }) => {
  const LINK_PASSWORD = 'link password 7f3a91';
  const requests = watchRequests(context);
  await signUp(page, uniqueName('locker'), PASSWORD);
  await upload(page, { [NAME]: SECRET });

  await rowMenu(page, NAME, 'Public link');
  const dialog = page.locator('dialog[open]');
  await dialog.locator('#link-password').fill(LINK_PASSWORD);
  await dialog.getByRole('button', { name: 'Create link' }).click();
  const url = (await dialog.locator('.font-mono', { hasText: '/s/' }).first().textContent()).trim();
  // A secret, not the key: the key needs the password too.
  const secret = url.split('#p.')[1];
  expect(secret?.length).toBeGreaterThan(20);

  const visitor = await browser.newContext({ colorScheme: 'dark' });
  const visitorRequests = watchRequests(visitor);
  const v = await visitor.newPage();
  await v.goto(url);
  await v.getByLabel('Password').fill('not the password');
  await v.getByRole('button', { name: 'Unlock' }).click();
  await expect(v.getByText('That password is not right.')).toBeVisible();
  await v.getByLabel('Password').fill(LINK_PASSWORD);
  await v.getByRole('button', { name: 'Unlock' }).click();
  await expect(v.getByRole('heading', { name: NAME })).toBeVisible();
  await v.getByRole('button', { name: 'Preview' }).click();
  await expect(v.locator('dialog.preview').getByText(SECRET)).toBeVisible();
  await visitor.close();

  for (const r of [requests, visitorRequests]) await r.expectNone([SECRET, NAME, PASSWORD, LINK_PASSWORD, 'not the password', secret]);
});

test('sharing with another person checks the fingerprint, and they can open it', async ({ page, context, browser }) => {
  const requests = watchRequests(context);
  const bobName = uniqueName('bob');
  const bobContext = await browser.newContext({ colorScheme: 'dark' });
  const bobRequests = watchRequests(bobContext);
  const bob = await bobContext.newPage();
  await signUp(bob, bobName, PASSWORD);

  await signUp(page, uniqueName('carol'), PASSWORD);
  await upload(page, { [NAME]: SECRET });
  await rowMenu(page, NAME, 'Share');
  const dialog = page.locator('dialog[open]');
  await dialog.getByLabel('Username').fill(bobName);
  await dialog.getByRole('button', { name: 'Continue' }).click();
  // The fingerprint must be confirmed before anything is sealed to the key.
  const share = dialog.getByRole('button', { name: 'Share', exact: true });
  await expect(share).toBeDisabled();
  await dialog.getByLabel('The fingerprints match').check();
  await share.click();
  await expect(page.getByText(`Shared with ${bobName}`)).toBeVisible();

  // A file shared on its own downloads, decrypted in Bob's browser.
  await bob.getByRole('button', { name: 'Shared with me' }).first().click();
  const download = bob.waitForEvent('download');
  await bob.getByRole('button', { name: new RegExp(NAME) }).first().click();
  const saved = await download;
  expect(saved.suggestedFilename()).toBe(NAME);
  const chunks = [];
  for await (const c of await saved.createReadStream()) chunks.push(c);
  expect(Buffer.concat(chunks).toString()).toBe(SECRET);
  await bobContext.close();

  for (const r of [requests, bobRequests]) await r.expectNone([SECRET, NAME, PASSWORD]);
});
