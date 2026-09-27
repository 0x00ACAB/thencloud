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
// Packs (Minimal, the default, is Lucide and needs nothing here):
//   material   Material Icon Theme (MIT), npm material-icon-theme
//   symbols    Symbols by Miguel Solorio (MIT), npm vscode-symbols: quiet,
//              muted icons
//   documents  file-icon-vectors "vivid" (MIT), npm file-icon-vectors:
//              document-shaped icons labelled with the file type
import { existsSync, readFileSync, readdirSync } from 'node:fs';
import { createRequire } from 'node:module';
import { dirname, join } from 'node:path';

const require = createRequire(import.meta.url);
const pkgDir = (name) => dirname(require.resolve(`${name}/package.json`));
const readJson = (path) => JSON.parse(readFileSync(path, 'utf8'));
const lower = (o = {}) => Object.fromEntries(Object.entries(o).map(([k, v]) => [k.toLowerCase(), v]));

function usedIds(t) {
  return new Set([t.fallback, ...Object.values(t.mime), ...Object.values(t.names), ...Object.values(t.extensions), ...Object.values(t.light)]);
}

/**
 * A VS Code icon theme (the format Material and Symbols use): fileNames,
 * fileExtensions, an optional `light` section, and iconDefinitions with
 * paths relative to the theme file.
 */
function vscodeTheme(themeFile, mime) {
  const base = dirname(themeFile);
  const m = readJson(themeFile);
  const names = lower(m.fileNames);
  const extensions = lower(m.fileExtensions);
  // Light-theme overrides by name/extension become id -> light id, so every
  // file using that icon gets the variant.
  const light = {};
  for (const [k, id] of Object.entries(lower(m.light?.fileNames))) if (names[k]) light[names[k]] = id;
  for (const [k, id] of Object.entries(lower(m.light?.fileExtensions))) if (extensions[k]) light[extensions[k]] = id;
  const tables = { names, extensions, light, fallback: m.file, mime };
  const svgs = new Map();
  for (const id of usedIds(tables)) {
    // Trimmed: a path in Symbols has a stray trailing space. Icons whose
    // file is missing are left out (the lookup then falls back).
    const path = m.iconDefinitions[id] && join(base, m.iconDefinitions[id].iconPath.trim());
    if (path && existsSync(path)) svgs.set(id, () => readFileSync(path));
  }
  return { tables, svgs };
}

const material = () =>
  vscodeTheme(join(pkgDir('material-icon-theme'), 'dist/material-icons.json'), { image: 'image', video: 'video', audio: 'audio' });

const symbols = () =>
  vscodeTheme(join(pkgDir('vscode-symbols'), 'src/symbol-icon-theme.json'), { image: 'image', video: 'video', audio: 'audio' });

/**
 * Our CSP (sent with every response, SVGs included) blocks <style> inside
 * an SVG, so turn simple class rules like `.st0{fill:#c11e07}` into
 * presentation attributes on the elements that use them.
 */
function inlineStyles(svg) {
  const style = svg.match(/<style[^>]*>([\s\S]*?)<\/style>/);
  if (!style) return svg;
  const rules = {};
  for (const [, selectors, body] of style[1].matchAll(/([^{}]+)\{([^}]*)\}/g)) {
    const decls = body
      .split(';')
      .map((d) => d.split(':').map((x) => x.trim()))
      .filter(([k, v]) => k && v);
    for (const sel of selectors.split(',')) {
      const cls = sel.trim().match(/^\.([\w-]+)$/)?.[1];
      if (cls) rules[cls] = [...(rules[cls] ?? []), ...decls];
    }
  }
  return svg.replace(style[0], '').replace(/class="([^"]*)"/g, (_, classes) =>
    classes
      .split(/\s+/)
      .flatMap((c) => rules[c] ?? [])
      .map(([k, v]) => `${k}="${v}"`)
      .join(' '),
  );
}

function documents() {
  const dir = join(pkgDir('file-icon-vectors'), 'dist/icons/vivid');
  const ids = readdirSync(dir)
    .filter((f) => f.endsWith('.svg'))
    .map((f) => f.slice(0, -4));
  // One icon per extension, named after it.
  const extensions = Object.fromEntries(ids.filter((id) => id !== 'blank').map((id) => [id.toLowerCase(), id]));
  const tables = { names: {}, extensions, light: {}, fallback: 'blank', mime: { image: 'image', video: 'mp4', audio: 'mp3' } };
  const svgs = new Map(ids.map((id) => [id, () => inlineStyles(readFileSync(join(dir, `${id}.svg`), 'utf8'))]));
  return { tables, svgs };
}

const PACKS = { material, symbols, documents };
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
