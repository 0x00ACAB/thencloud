// File-type icon packs, chosen in Settings.
//
// For each pack this provides a virtual module, `virtual:file-icons/<pack>`,
// with lookup tables in one shared shape:
//
//   { names, extensions, light, fallback, mime }
//
// names/extensions map a lower-case file name or extension (possibly
// compound, like "d.ts") to an icon id; light maps an icon id to its
// variant for light backgrounds; mime has generic image/video/audio ids.
//
// The SVGs are served from /file-icons/<pack>/<id>.svg (and copied into
// dist/ on build) so they come from our own origin like everything else.
// The client imports a pack's tables only when that pack is chosen, and the
// browser fetches only the icons it shows. Folder icons aren't used.
//
// Packs:
//   material  Material Icon Theme (MIT), npm material-icon-theme
//   vscode    vscode-icons (MIT): tables from vscode-icons-js, SVGs from @iconify-json/vscode-icons
//   seti      Seti UI (MIT), VS Code's default, vendored in vendor/seti/
import { readFileSync } from 'node:fs';
import { createRequire } from 'node:module';
import { dirname, join } from 'node:path';

const require = createRequire(import.meta.url);
const pkgDir = (name) => dirname(require.resolve(`${name}/package.json`));
const readJson = (path) => JSON.parse(readFileSync(path, 'utf8'));
const lower = (o = {}) => Object.fromEntries(Object.entries(o).map(([k, v]) => [k.toLowerCase(), v]));

// Each loader returns { tables, svgs: Map<id, () => svg source> }.

function material() {
  const root = pkgDir('material-icon-theme');
  const m = readJson(join(root, 'dist/material-icons.json'));
  const names = lower(m.fileNames);
  const extensions = lower(m.fileExtensions);
  // The pack overrides a few names/extensions for light themes; turn those
  // into id -> light id so every file using that icon gets the variant.
  const light = {};
  for (const [k, id] of Object.entries(lower(m.light?.fileNames))) if (names[k]) light[names[k]] = id;
  for (const [k, id] of Object.entries(lower(m.light?.fileExtensions))) if (extensions[k]) light[extensions[k]] = id;
  const tables = { names, extensions, light, fallback: m.file, mime: { image: 'image', video: 'video', audio: 'audio' } };
  const svgs = new Map();
  for (const id of usedIds(tables)) {
    const def = m.iconDefinitions[id];
    if (def) svgs.set(id, () => readFileSync(join(root, 'dist', def.iconPath)));
  }
  return { tables, svgs };
}

function vscode() {
  const gen = join(pkgDir('vscode-icons-js'), 'dist/generated');
  const table = (file, key) => lower(require(join(gen, file))[key]);
  const strip = (o) => Object.fromEntries(Object.entries(o).map(([k, v]) => [k, v.replace(/\.svg$/, '')]));
  // Like vscode-icons-js: exact extensions first, then the per-language table
  // (keyed by extension too) as a fallback.
  const extensions = strip({
    ...table('LanguagesToIcon.js', 'LanguagesToIcon'),
    ...table('FileExtensions1ToIcon.js', 'FileExtensions1ToIcon'),
    ...table('FileExtensions2ToIcon.js', 'FileExtensions2ToIcon'),
  });
  const names = strip(table('FileNamesToIcon.js', 'FileNamesToIcon'));

  const set = readJson(join(pkgDir('@iconify-json/vscode-icons'), 'icons.json'));
  const icon = (id) => {
    let name = id.replaceAll('_', '-');
    let props = {};
    for (let hops = 0; !set.icons[name] && set.aliases?.[name] && hops < 5; hops++) {
      props = { ...set.aliases[name], ...props };
      name = set.aliases[name].parent;
    }
    const i = set.icons[name];
    if (!i) return null;
    const w = props.width ?? i.width ?? set.width ?? 16;
    const h = props.height ?? i.height ?? set.height ?? 16;
    return `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 ${w} ${h}">${i.body}</svg>`;
  };

  // Some entries point straight at a light-theme variant; use the base icon
  // and let `light` pick the variant.
  for (const t of [names, extensions]) {
    for (const [k, id] of Object.entries(t)) {
      const base = id.replace(/^file_type_light_/, 'file_type_');
      if (base !== id && icon(base)) t[k] = base;
    }
  }

  const tables = {
    names,
    extensions,
    light: {},
    fallback: 'default_file',
    mime: { image: 'file_type_image', video: 'file_type_video', audio: 'file_type_audio' },
  };
  for (const id of usedIds(tables)) {
    const lightId = id.replace(/^file_type_(?!light_)/, 'file_type_light_');
    if (lightId !== id && icon(lightId)) tables.light[id] = lightId;
  }
  const svgs = new Map();
  for (const id of usedIds(tables)) {
    const svg = icon(id);
    if (svg) svgs.set(id, () => svg);
  }
  return { tables, svgs };
}

// Seti's colour palette, and darker shades for the two that don't read on
// a light background.
const SETI_COLOURS = {
  blue: '#519aba', grey: '#4d5a5e', 'grey-light': '#6d8086', green: '#8dc149', orange: '#e37933', pink: '#f55385',
  purple: '#a074c4', red: '#cc3e44', white: '#d4d7d6', yellow: '#cbcb41', ignore: '#41535b',
};
const SETI_LIGHT = { white: '#6d8086', yellow: '#a3a32e' };

function seti() {
  const vendor = new URL('../vendor/seti/', import.meta.url);
  const defs = readJson(new URL('definitions.json', vendor));
  const glyphs = readJson(new URL('icons.json', vendor));
  const id = ([glyph, colour]) => `${glyph}-${colour}`;
  const names = {};
  for (const [k, v] of Object.entries(defs.files)) names[k.toLowerCase()] = id(v);
  const extensions = {};
  for (const [k, v] of Object.entries(defs.extensions)) extensions[k.replace(/^\./, '').toLowerCase()] = id(v);
  const tables = {
    names,
    extensions,
    light: {},
    fallback: id(defs.default),
    mime: { image: 'image-purple', video: 'video-pink', audio: 'audio-purple' },
  };
  const svgs = new Map();
  // Longest colour names first, so "grey-light" isn't read as "grey".
  const colours = Object.keys(SETI_COLOURS).sort((x, y) => y.length - x.length);
  for (const i of usedIds(tables)) {
    const colour = colours.find((c) => i.endsWith(`-${c}`));
    const glyph = colour && glyphs[i.slice(0, -colour.length - 1)];
    if (!glyph) continue;
    const svg = (fill) => glyph.replace('<svg ', `<svg xmlns="http://www.w3.org/2000/svg" fill="${fill}" `);
    svgs.set(i, () => svg(SETI_COLOURS[colour]));
    if (SETI_LIGHT[colour]) {
      tables.light[i] = `${i}-light`;
      svgs.set(`${i}-light`, () => svg(SETI_LIGHT[colour]));
    }
  }
  return { tables, svgs };
}

function usedIds(t) {
  return new Set([t.fallback, ...Object.values(t.mime), ...Object.values(t.names), ...Object.values(t.extensions), ...Object.values(t.light)]);
}

const PACKS = { material, vscode, seti };
const PREFIX = 'virtual:file-icons/';

export default function fileIcons() {
  const cache = {};
  const pack = (name) => (cache[name] ??= PACKS[name]());
  return {
    name: 'thencloud-file-icons',
    resolveId: (id) => (id.startsWith(PREFIX) && PACKS[id.slice(PREFIX.length)] ? '\0' + id : null),
    load(id) {
      if (!id.startsWith('\0' + PREFIX)) return null;
      return `export default ${JSON.stringify(pack(id.slice(PREFIX.length + 1)).tables)};`;
    },
    configureServer(server) {
      server.middlewares.use((req, res, next) => {
        const m = req.url?.match(/^\/file-icons\/([a-z]+)\/([^/?]+)\.svg/);
        const svg = m && PACKS[m[1]] && pack(m[1]).svgs.get(decodeURIComponent(m[2]));
        if (!svg) return next();
        res.setHeader('Content-Type', 'image/svg+xml');
        res.end(svg());
      });
    },
    generateBundle() {
      for (const name of Object.keys(PACKS)) {
        for (const [id, svg] of pack(name).svgs) this.emitFile({ type: 'asset', fileName: `file-icons/${name}/${id}.svg`, source: svg() });
      }
    },
  };
}
