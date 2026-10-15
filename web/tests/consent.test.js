// The linking popup only goes to Google's consent page (or the browser
// tests' stand-in on loopback), whatever address the server sends.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { consentUrlAllowed } from '../src/lib/consent.js';
import { random, mutate, randomText, RUNS, SEED } from './fuzz.js';

test('consent: Google and loopback only', () => {
  assert.equal(consentUrlAllowed('https://accounts.google.com/o/oauth2/v2/auth?client_id=x&state=y'), true);
  assert.equal(consentUrlAllowed('http://127.0.0.1:8094/o/oauth2/v2/auth?state=y'), true);
  assert.equal(consentUrlAllowed('http://localhost:8094/auth'), true);
  for (const bad of [
    'http://accounts.google.com/o/oauth2/v2/auth',
    'https://accounts.google.com.evil.example/auth',
    'https://evil.example/?https://accounts.google.com/',
    'https://user:pass@accounts.google.com/auth',
    'https://accounts.google.com:8443/auth',
    'https://127.0.0.1/auth',
    'http://10.0.0.1/auth',
    'javascript:alert(1)',
    'data:text/html,hi',
    '//accounts.google.com/auth',
    '',
    undefined,
  ]) {
    assert.equal(consentUrlAllowed(bad), false, String(bad));
  }
});

test(`consent: mutated addresses pass only to Google or loopback (seed ${SEED})`, () => {
  const rnd = random(SEED + 31);
  const seeds = ['https://accounts.google.com/o/oauth2/v2/auth?state=y', 'http://127.0.0.1:8094/auth'].map((u) => new TextEncoder().encode(u));
  const dec = new TextDecoder();
  for (let i = 0; i < RUNS; i++) {
    const s = rnd.int(4) ? dec.decode(mutate(rnd.pick(seeds), rnd)) : randomText(rnd, 80);
    if (!consentUrlAllowed(s)) continue;
    const u = new URL(s);
    const ok = (u.protocol === 'https:' && u.host === 'accounts.google.com') || (u.protocol === 'http:' && ['127.0.0.1', 'localhost', '[::1]'].includes(u.hostname));
    assert.ok(ok, `${JSON.stringify(s)} -> ${u.href}`);
  }
});
