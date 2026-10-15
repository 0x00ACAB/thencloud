// A video this browser can't show (HEVC: Playwright's Chromium has no
// decoder for it) gets an offer to convert it here, and the converted copy
// shows a picture. Nothing about it reaches the server but ciphertext.
import { test, expect } from '@playwright/test';
import { readFileSync } from 'node:fs';
import { watchRequests, uniqueName, signUp, upload } from './helpers.js';

const PASSWORD = 'correct horse battery e2e';
const NAME = 'Holiday 5c08d2.mp4';

for (const h264 of [true, false]) {
  test(`a video the browser can't play is converted for watching (${h264 ? 'to H.264' : 'to VP8, without H.264'})`, async ({ page, context }) => {
    test.setTimeout(240_000);
    // A browser without H.264 (open-source Chromium, some Linux builds) gets WebM instead.
    if (!h264) {
      await page.addInitScript(() => {
        const real = HTMLMediaElement.prototype.canPlayType;
        HTMLMediaElement.prototype.canPlayType = function (type) {
          return /avc1|h264/i.test(type) ? '' : real.call(this, type);
        };
      });
    }
    const requests = watchRequests(context);
    await signUp(page, uniqueName('vida'), PASSWORD);
    await upload(page, { [NAME]: readFileSync(new URL('./fixtures/hevc.mp4', import.meta.url)) });

    await page.locator('button.row-open', { hasText: NAME }).click();
    const preview = page.locator('dialog.preview');
    await expect(preview.getByText("Your browser can't play this video's format.")).toBeVisible({ timeout: 30_000 });
    await preview.getByRole('button', { name: 'Convert to watch' }).click();

    // ffmpeg.wasm loads (about 30 MB) and converts the two seconds.
    const video = preview.locator('video');
    await expect(video).toBeVisible({ timeout: 180_000 });
    await expect.poll(() => video.evaluate((v) => v.videoWidth), { timeout: 30_000 }).toBeGreaterThan(0);
    expect(await video.evaluate((v) => v.src.startsWith('blob:'))).toBe(true);

    await requests.expectNone([NAME, 'Holiday 5c08d2', PASSWORD]);
  });
}
