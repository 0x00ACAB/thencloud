// Series, season, episode and title from a video's name and folders:
// "Show S01E02 Title.mkv", "Show.Name.1x02.Title.720p.mkv",
// "Show/Season 1/02 - Title.mp4", and our own "Title S01E02.mp4" inside a
// folder named after the show. Anything else is a film: { title }.

const JUNK =
  /\b(2160p|1080p|720p|576p|480p|4k|uhd|hdr\d*|x26[45]|h\.?26[45]|hevc|avc|xvid|web[- ]?(dl|rip)?|blu[- ]?ray|bdrip|brrip|dvdrip|hdtv|remux|aac[\d.]*|ac3|dts|ddp?[\d.]*|atmos|10bit|proper|repack|internal|multi)\b.*$/i;
const SEASON_DIR = /^(?:season|series|staffel|saison|s)\s*(\d{1,3})$|^specials?$/i;
const CODE = [/^(.*?)\bS(\d{1,3})[ ._-]?E(\d{1,4})(?:[ ._-]?-?E\d{1,4})*\b(.*)$/i, /^(.*?)\b(\d{1,2})x(\d{2,3})\b(.*)$/i];
const IN_SEASON = [/^(?:e|ep\.?|episode)\s*(\d{1,3})\b(.*)$/i, /^(\d{1,3})(?=[\s\-._)]|$)(.*)$/];

export function clean(s = '') {
  if (!/\s/.test(s)) s = s.replace(/[._]+/g, ' ');
  return s
    .replace(/_/g, ' ')
    .replace(/\[[^\]]*\]/g, ' ')
    .replace(JUNK, '')
    .replace(/^[\s\-–.:)]+|[\s\-–.:(]+$/g, '')
    .replace(/\s{2,}/g, ' ');
}

const same = (a, b) => clean(a).toLowerCase() === clean(b).toLowerCase();

/** `folders` are the folder names from the library root (not included) down to the file's. */
export function parseEpisode(name, folders = []) {
  const base = name.replace(/\.[^.]+$/, '');
  const parent = folders.at(-1);
  const seasonDir = parent?.match(SEASON_DIR);
  const show = clean((seasonDir ? folders.at(-2) : parent) ?? '');

  let m;
  for (const re of CODE) if ((m = base.match(re))) break;
  if (m) {
    const [, before, season, episode, after] = m;
    let series = clean(before) || show;
    let title = clean(after);
    // "Title S01E02" in the show's folder: what's before the code is the title.
    if (!title && before && show && !same(before, show) && !clean(before).toLowerCase().startsWith(show.toLowerCase())) {
      title = clean(before);
      series = show;
    }
    return { series, season: +season, episode: +episode, title };
  }
  if (seasonDir) {
    for (const re of IN_SEASON) {
      const e = base.match(re);
      if (e) return { series: show, season: seasonDir[1] ? +seasonDir[1] : 0, episode: +e[1], title: clean(e[2]) };
    }
  }
  return { title: clean(base) || base };
}

/** Episode details from a title such as a Matroska file's: { series, season, episode, title } or { title }. */
export const fromTitle = (title) => parseEpisode(`${title}.x`);

export const pad = (n) => String(n ?? 0).padStart(2, '0');

/** "{Episode Name} S01E02". */
// `untitled` names an episode without a title ("Episode 2", in the reader's language).
export const episodeLabel = (e, untitled = `Episode ${e.episode}`) => `${e.title || untitled} S${pad(e.season)}E${pad(e.episode)}`;

/** `s` without the characters file systems refuse. */
export const safeName = (s) => s.replace(/[\\/:*?"<>|\u0000-\u001f]+/g, ' ').replace(/\s{2,}/g, ' ').trim();
