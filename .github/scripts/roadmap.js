// Syncs MILESTONES.md to GitHub: each "## " section becomes a milestone and
// each unchecked item an issue labelled "roadmap". Ticking an item closes its
// issue. MILESTONES.md stays the source of truth.
const fs = require('fs');

const LABEL = 'roadmap';

function parse(markdown) {
  const sections = [];
  let section, parent;
  for (const line of markdown.split('\n')) {
    const heading = line.match(/^## (.+)/);
    if (heading) {
      const title = heading[1]
        .replace(/^[^\p{L}\p{N}]+/u, '')
        .replace(/\s*\([^)]*\)\s*$/, '')
        .trim();
      section = { title, items: [] };
      sections.push(section);
      continue;
    }
    const item = line.match(/^(\s*)- \[([ xX])\] (.+)/);
    if (!item || !section) continue;
    const [, indent, mark, text] = item;
    const bold = text.match(/^\*\*(.+?)\*\*/);
    let title = (bold ? bold[1] : text.replace(/\s*\([^)]*\)/g, '')).replace(/[.:]$/, '').trim();
    if (indent.length === 0) parent = bold ? bold[1] : null;
    else if (parent) title = `${parent}: ${title}`;
    section.items.push({ title, text, done: mark !== ' ', id: slug(title) });
  }
  return sections;
}

function slug(s) {
  return s.toLowerCase().replace(/[^a-z0-9]+/g, '-').replace(/^-|-$/g, '').slice(0, 80);
}

const marker = (id) => `<!-- roadmap:${id} -->`;

module.exports = async ({ github, context, core, dryRun = false, file = 'MILESTONES.md' }) => {
  const { owner, repo } = context.repo;
  const sections = parse(fs.readFileSync(file, 'utf8'));
  const act = async (what, fn) => {
    core.info(`${dryRun ? '[dry run] ' : ''}${what}`);
    return dryRun ? null : fn();
  };

  try {
    await github.rest.issues.getLabel({ owner, repo, name: LABEL });
  } catch {
    await act(`create label ${LABEL}`, () =>
      github.rest.issues.createLabel({ owner, repo, name: LABEL, color: '3B47F9', description: 'Tracked in MILESTONES.md' }));
  }

  const milestones = await github.paginate(github.rest.issues.listMilestones, { owner, repo, state: 'all', per_page: 100 });
  const issues = await github.paginate(github.rest.issues.listForRepo, { owner, repo, labels: LABEL, state: 'all', per_page: 100 });
  const byId = new Map();
  for (const issue of issues) {
    const m = issue.body?.match(/<!-- roadmap:([a-z0-9-]+) -->/);
    if (m) byId.set(m[1], issue);
  }

  for (const section of sections) {
    const state = section.items.some((i) => !i.done) ? 'open' : 'closed';
    let milestone = milestones.find((m) => m.title === section.title);
    if (!milestone) {
      milestone = await act(`create milestone "${section.title}" (${state})`, async () =>
        (await github.rest.issues.createMilestone({ owner, repo, title: section.title, state, description: 'From MILESTONES.md' })).data);
    } else if (milestone.state !== state) {
      await act(`mark milestone "${section.title}" ${state}`, () =>
        github.rest.issues.updateMilestone({ owner, repo, milestone_number: milestone.number, state }));
    }
    const number = milestone?.number;

    for (const item of section.items) {
      const issue = byId.get(item.id);
      if (!issue) {
        if (item.done) continue;
        const body = `${item.text}\n\nFrom the **${section.title}** section of [MILESTONES.md](../blob/HEAD/MILESTONES.md). Tick the item there to close this issue.\n\n${marker(item.id)}`;
        await act(`open "${item.title}"`, () =>
          github.rest.issues.create({ owner, repo, title: item.title, body, labels: [LABEL], milestone: number }));
      } else if (item.done && issue.state === 'open') {
        await act(`close #${issue.number} "${item.title}"`, () =>
          github.rest.issues.update({ owner, repo, issue_number: issue.number, state: 'closed', state_reason: 'completed' }));
      } else if (number && issue.milestone?.number !== number) {
        await act(`move #${issue.number} to "${section.title}"`, () =>
          github.rest.issues.update({ owner, repo, issue_number: issue.number, milestone: number }));
      }
    }
  }
};

module.exports.parse = parse;
