// Secure Send (Settings): a one-off piece of text shared as a counted
// public link. Checks the text and the link's key never leave the
// browser, and that the link really does stop working after one open.
import { test, expect } from '@playwright/test';
import { watchRequests, uniqueName, signUp } from './helpers.js';

const PASSWORD = 'correct horse battery e2e';
const SECRET_TEXT = 'TOP-SECRET-MESSAGE-9c2f14';

test('Secure Send shares text as a one-time link that stops working after it', async ({ page, context, browser }) => {
  const requests = watchRequests(context);
  await signUp(page, uniqueName('sender'), PASSWORD);

  await page.getByRole('button', { name: 'Settings', exact: true }).first().click();
  await page.getByRole('button', { name: 'Secure Send', exact: true }).click();

  const dialog = page.locator('dialog[open]');
  await dialog.getByLabel('Message').fill(SECRET_TEXT);
  await dialog.getByRole('button', { name: 'Create link' }).click();
  const url = (await dialog.locator('.font-mono', { hasText: '/s/' }).first().textContent()).trim();
  const key = url.split('#')[1];
  expect(key.length).toBeGreaterThan(20);
  await dialog.getByRole('button', { name: 'Done' }).click();

  // A visitor with no account opens it once.
  const visitor = await browser.newContext({ colorScheme: 'dark' });
  const visitorRequests = watchRequests(visitor);
  const v = await visitor.newPage();
  await v.goto(url);
  await expect(v.getByRole('heading', { name: 'Message.txt' })).toBeVisible();
  await v.getByRole('button', { name: 'Preview' }).click();
  await expect(v.locator('dialog.preview').getByText(SECRET_TEXT)).toBeVisible();
  await visitor.close();

  // Used up: a second visitor finds nothing, even with the same key.
  const second = await browser.newContext();
  const secondRequests = watchRequests(second);
  const v2 = await second.newPage();
  await v2.goto(url);
  await expect(v2.getByText('This link has expired or was removed')).toBeVisible();
  await second.close();

  for (const r of [requests, visitorRequests, secondRequests]) await r.expectNone([SECRET_TEXT, key]);
});
