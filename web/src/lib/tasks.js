// Tick or untick the n-th task list item ("- [ ] ...") in Markdown source,
// counting like the renderer: items inside fenced code blocks don't count.

const TASK = /^(\s*(?:[-*+]|\d+[.)])\s+\[)([ xX])(\])/;
const FENCE = /^\s*(```|~~~)/;

/** The text with task `index` set to `checked`, or null if there's no such task. */
export function setTask(text, index, checked) {
  const lines = text.split('\n');
  let fence = null;
  let n = 0;
  for (let i = 0; i < lines.length; i++) {
    const f = lines[i].match(FENCE);
    if (f) {
      if (!fence) fence = f[1];
      else if (f[1] === fence) fence = null;
      continue;
    }
    if (fence) continue;
    const m = lines[i].match(TASK);
    if (!m) continue;
    if (n++ === index) {
      lines[i] = lines[i].replace(TASK, `$1${checked ? 'x' : ' '}$3`);
      return lines.join('\n');
    }
  }
  return null;
}
