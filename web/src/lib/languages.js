// File name to highlight.js language. Kept apart from highlight.js itself
// so the file list can use it without loading the highlighter.

const BY_EXTENSION = {
  sh: 'bash', bash: 'bash', zsh: 'bash', fish: 'bash', env: 'bash',
  c: 'c', h: 'c', cc: 'cpp', cpp: 'cpp', cxx: 'cpp', hpp: 'cpp', cs: 'csharp',
  css: 'css', scss: 'scss', sass: 'scss', less: 'scss',
  diff: 'diff', patch: 'diff', dockerfile: 'dockerfile', go: 'go', graphql: 'graphql', gql: 'graphql',
  hs: 'haskell', ini: 'ini', cfg: 'ini', conf: 'ini', toml: 'ini', properties: 'ini', editorconfig: 'ini',
  java: 'java', gradle: 'java', kt: 'kotlin', kts: 'kotlin',
  js: 'javascript', mjs: 'javascript', cjs: 'javascript', jsx: 'javascript', json: 'json', jsonc: 'json', json5: 'json',
  tex: 'latex', lua: 'lua', mk: 'makefile', makefile: 'makefile', md: 'markdown', markdown: 'markdown', nix: 'nix',
  php: 'php', ps1: 'powershell', proto: 'protobuf', py: 'python', rb: 'ruby', rs: 'rust', scala: 'scala',
  sql: 'sql', swift: 'swift', ts: 'typescript', mts: 'typescript', cts: 'typescript', tsx: 'typescript',
  html: 'xml', htm: 'xml', xml: 'xml', svg: 'xml', svelte: 'xml', vue: 'xml', yaml: 'yaml', yml: 'yaml',
};
const BY_NAME = { dockerfile: 'dockerfile', makefile: 'makefile', justfile: 'makefile' };

/** highlight.js language for a file name, or null for plain text. */
export function languageFor(name) {
  const lower = name.toLowerCase();
  if (BY_NAME[lower]) return BY_NAME[lower];
  const i = lower.lastIndexOf('.');
  return i > 0 ? (BY_EXTENSION[lower.slice(i + 1)] ?? null) : null;
}
