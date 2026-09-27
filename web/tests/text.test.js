// Text-shaped untrusted input: CSV, subtitles, episode names.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { parseCsv, compareCells } from '../src/lib/csv.js';
import { matchSubtitles, toVtt } from '../src/lib/subtitles.js';
import { parseEpisode, safeName } from '../src/lib/episodes.js';
import { random, randomText, timed, RUNS, SEED } from './fuzz.js';

test('CSV: quotes, separators and newlines inside fields', () => {
  const { rows } = parseCsv('﻿a,b\r\n1,"x, ""y""\nz"\n', ',');
  assert.deepEqual(rows, [['a', 'b'], ['1', 'x, "y"\nz']]);
});

test(`CSV: random text parses quickly into rows of strings (seed ${SEED})`, async () => {
  const rnd = random(SEED + 3);
  for (let i = 0; i < RUNS; i++) {
    const text = randomText(rnd, 400);
    const { rows } = await timed(() => parseCsv(text, rnd.pick([',', '\t']), 1 + rnd.int(50)));
    for (const r of rows) for (const c of r) assert.equal(typeof c, 'string');
    const cells = rows.flat();
    await timed(() => [...cells].sort(compareCells));
  }
  // A long unterminated quote is linear, not quadratic.
  await timed(() => parseCsv('"' + 'a,'.repeat(200000), ','), 1000);
});

test(`subtitles: names match their video, SRT becomes WebVTT (seed ${SEED})`, async () => {
  const f = (name) => ({ meta: { name, size: 10 } });
  const m = matchSubtitles('Film.mp4', [f('Film.en.srt'), f('Film 2.srt'), f('Film.sdh.srt'), f('Filmx.vtt')]);
  assert.deepEqual(m.map((t) => t.entry.meta.name).sort(), ['Film.en.srt', 'Film.sdh.srt']);
  assert.ok(toVtt('1\n00:00:01,000 --> 00:00:02,000\nHi\n').startsWith('WEBVTT\n\n1\n00:00:01.000'));
  const rnd = random(SEED + 4);
  for (let i = 0; i < RUNS; i++) {
    const video = randomText(rnd, 30) + '.mp4';
    const names = Array.from({ length: rnd.int(6) }, () => f(randomText(rnd, 40) + rnd.pick(['.srt', '.vtt', '.en.srt', ''])));
    const out = await timed(() => matchSubtitles(video, names));
    for (const t of out) assert.equal(typeof t.label, 'string');
    const vtt = await timed(() => toVtt(randomText(rnd, 500)));
    assert.ok(vtt.startsWith('WEBVTT'));
  }
});

test(`episodes: names are read quickly whatever they hold (seed ${SEED})`, async () => {
  assert.equal(parseEpisode('Show S01E02 Title.mkv').episode, 2);
  const rnd = random(SEED + 5);
  for (let i = 0; i < RUNS; i++) {
    const name = randomText(rnd, 120);
    const e = await timed(() => parseEpisode(name, [randomText(rnd, 20)]));
    assert.equal(typeof e, 'object');
    assert.ok(!/[\\/:*?"<>|]/.test(safeName(name)));
  }
  // Long runs of the characters the patterns look for.
  await timed(() => parseEpisode('S0'.repeat(20000) + 'x'), 1000);
  await timed(() => parseEpisode('1x'.repeat(20000) + ' '), 1000);
});
