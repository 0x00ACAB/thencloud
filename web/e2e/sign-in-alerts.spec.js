// A sign-in on another device shows up as an alert on this one, which can
// sign it out. Which alerts were seen is kept in the encrypted app data.
import { test, expect } from '@playwright/test';
import { watchRequests, uniqueName, signUp } from './helpers.js';

const PASSWORD = 'correct horse battery e2e';

test('a new sign-in elsewhere is shown, and can be signed out from here', async ({ page, context, browser }) => {
  const requests = watchRequests(context);
  const username = uniqueName('sina');
  await signUp(page, username, PASSWORD);
  // Nothing to report about the account's own first session.
  await expect(page.getByText('New sign-in to your account')).toHaveCount(0);

  // Another device signs in with the password.
  const other = await browser.newContext();
  const phone = await other.newPage();
  await phone.goto('/');
  await phone.getByLabel('Username').fill(username);
  await phone.getByLabel('Password', { exact: true }).fill(PASSWORD);
  await phone.getByRole('button', { name: 'Sign in', exact: true }).click();
  await expect(phone.getByRole('heading', { name: 'My files' })).toBeVisible({ timeout: 60_000 });

  // Back here, the next check (the tab coming back into view) shows it.
  await page.evaluate(() => document.dispatchEvent(new Event('visibilitychange')));
  const alert = page.locator('[aria-label="New sign-ins"] .card');
  await expect(alert).toHaveCount(1, { timeout: 15_000 });
  await expect(alert).toContainText('with your password');

  // Signing it out ends the other device's session.
  await alert.getByRole('button', { name: 'Sign it out' }).click();
  await expect(alert).toHaveCount(0);
  // Its next request is refused, and it's back at the sign-in screen.
  await phone.evaluate(() => (location.hash = '#/settings'));
  await expect(phone.getByLabel('Username')).toBeVisible({ timeout: 15_000 });

  // And it stays dismissed at the next check.
  await page.evaluate(() => document.dispatchEvent(new Event('visibilitychange')));
  await expect(page.getByText('New sign-in to your account')).toHaveCount(0);

  await other.close();
  await requests.expectNone([PASSWORD]);
});
