// Syntax highlighting for text previews. Loaded on demand (import()), so the
// file list doesn't pay for it. highlight.js escapes the source itself and
// only adds <span class="hljs-..."> around it.

import hljs from 'highlight.js/lib/core';
import bash from 'highlight.js/lib/languages/bash';
import c from 'highlight.js/lib/languages/c';
import cpp from 'highlight.js/lib/languages/cpp';
import csharp from 'highlight.js/lib/languages/csharp';
import css from 'highlight.js/lib/languages/css';
import diff from 'highlight.js/lib/languages/diff';
import dockerfile from 'highlight.js/lib/languages/dockerfile';
import go from 'highlight.js/lib/languages/go';
import graphql from 'highlight.js/lib/languages/graphql';
import haskell from 'highlight.js/lib/languages/haskell';
import ini from 'highlight.js/lib/languages/ini';
import java from 'highlight.js/lib/languages/java';
import javascript from 'highlight.js/lib/languages/javascript';
import json from 'highlight.js/lib/languages/json';
import kotlin from 'highlight.js/lib/languages/kotlin';
import latex from 'highlight.js/lib/languages/latex';
import lua from 'highlight.js/lib/languages/lua';
import makefile from 'highlight.js/lib/languages/makefile';
import markdown from 'highlight.js/lib/languages/markdown';
import nix from 'highlight.js/lib/languages/nix';
import php from 'highlight.js/lib/languages/php';
import plaintext from 'highlight.js/lib/languages/plaintext';
import powershell from 'highlight.js/lib/languages/powershell';
import protobuf from 'highlight.js/lib/languages/protobuf';
import python from 'highlight.js/lib/languages/python';
import ruby from 'highlight.js/lib/languages/ruby';
import rust from 'highlight.js/lib/languages/rust';
import scala from 'highlight.js/lib/languages/scala';
import scss from 'highlight.js/lib/languages/scss';
import sql from 'highlight.js/lib/languages/sql';
import swift from 'highlight.js/lib/languages/swift';
import typescript from 'highlight.js/lib/languages/typescript';
import xml from 'highlight.js/lib/languages/xml';
import yaml from 'highlight.js/lib/languages/yaml';

const LANGUAGES = {
  bash, c, cpp, csharp, css, diff, dockerfile, go, graphql, haskell, ini, java, javascript, json, kotlin, latex,
  lua, makefile, markdown, nix, php, plaintext, powershell, protobuf, python, ruby, rust, scala, scss, sql, swift,
  typescript, xml, yaml,
};
for (const [name, lang] of Object.entries(LANGUAGES)) hljs.registerLanguage(name, lang);

/** Highlighted HTML for `text`. Safe to insert: all source text is escaped. */
export function highlight(text, language) {
  if (!language || !hljs.getLanguage(language)) return hljs.highlight(text, { language: 'plaintext' }).value;
  return hljs.highlight(text, { language, ignoreIllegals: true }).value;
}

/** Highlight fenced code blocks inside rendered Markdown in place. */
export function highlightBlocks(root) {
  for (const el of root.querySelectorAll('pre code[class*="language-"]')) {
    const lang = [...el.classList].find((c) => c.startsWith('language-'))?.slice(9);
    if (lang && hljs.getLanguage(lang)) hljs.highlightElement(el);
  }
}
