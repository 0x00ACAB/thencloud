// The next track in the queue is fetched and decrypted while the current one
// plays (lib/music.svelte.js, "getting the next track ready"), so moving on
// to it doesn't wait on the network.
import { test, expect } from '@playwright/test';
import { watchRequests, uniqueName, signUp, upload } from './helpers.js';

const PASSWORD = 'correct horse battery e2e';
const FIRST = '01 Dawn 4f2c71.wav';
const SECOND = '02 Dusk 4f2c71.wav';

// A mono 8 kHz WAV of `seconds` of a quiet tone: small, and every browser plays it.
function wav(seconds, hz) {
  const rate = 8000;
  const n = rate * seconds;
  const b = Buffer.alloc(44 + n * 2);
  b.write('RIFF', 0);
  b.writeUInt32LE(36 + n * 2, 4);
  b.write('WAVEfmt ', 8);
  b.writeUInt32LE(16, 16);
  b.writeUInt16LE(1, 20); // PCM
  b.writeUInt16LE(1, 22); // mono
  b.writeUInt32LE(rate, 24);
  b.writeUInt32LE(rate * 2, 28);
  b.writeUInt16LE(2, 32);
  b.writeUInt16LE(16, 34);
  b.write('data', 36);
  b.writeUInt32LE(n * 2, 40);
  for (let i = 0; i < n; i++) b.writeInt16LE(Math.round(2000 * Math.sin((2 * Math.PI * hz * i) / rate)), 44 + i * 2);
  return b;
}

test('the next track is ready before the current one ends', async ({ page, context }) => {
  test.setTimeout(90_000);
  const requests = watchRequests(context);
  await signUp(page, uniqueName('mira'), PASSWORD);
  await upload(page, { [FIRST]: wav(7, 440), [SECOND]: wav(7, 330) });

  // When each file version's first piece was asked for, by version id.
  const firstPiece = new Map();
  page.on('request', (r) => {
    const m = r.url().match(/\/versions\/([^/]+)\/chunks\/0$/);
    if (m && !firstPiece.has(m[1])) firstPiece.set(m[1], Date.now());
  });

  await page.locator('aside').getByRole('button', { name: 'Music' }).click();
  await page.getByRole('button', { name: 'Choose folder' }).first().click();
  await page.getByRole('button', { name: 'Use My files' }).click();
  await page.getByRole('button', { name: /^Play / }).first().click({ force: true });

  const bar = page.locator('.player-bar');
  await expect(bar).toContainText('Dawn 4f2c71');
  await expect(bar).toContainText('Dusk 4f2c71', { timeout: 30_000 });
  const switched = Date.now();

  // Two versions were read, the second one well before the first track ended.
  expect(firstPiece.size).toBe(2);
  const [, second] = [...firstPiece.values()].sort((a, b) => a - b);
  expect(switched - second).toBeGreaterThan(1500);

  // And the second track really plays.
  const position = bar.getByLabel('Position').first();
  await expect.poll(async () => Number(await position.inputValue()), { timeout: 10_000 }).toBeGreaterThan(0.5);
  await expect(bar.getByRole('button', { name: 'Pause' }).first()).toBeVisible();

  await requests.expectNone([FIRST, SECOND, 'Dawn 4f2c71', 'Dusk 4f2c71', PASSWORD]);
});
