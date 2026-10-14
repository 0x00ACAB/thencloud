# Accessibility

thencloud is where people keep their files, photos, music and notes. If you
can't sign in, find a file or share it because of how the app is built, it
has failed you, and we want to know. This page covers what we aim for, what
we've checked, what we know doesn't work well yet, and how to tell us about a
problem.

It applies to the web app, the public link page, the desktop and Android apps
(which show the same web app) and the `thencloud` command line.

## Priorities

We aim for the web app to meet WCAG 2.2 level AA. That's a goal we work
toward, not a claim: nobody has evaluated thencloud against it yet. What we
focus on:

- **Keyboard:** everything you can do with a mouse should work from the
  keyboard. Files can be moved, selected and opened from the keyboard, menus
  take arrow keys, Home and End, dialogs keep focus inside until closed and
  return it after, and `?` in the file list shows the shortcuts.
- **Screen readers:** buttons that only show an icon have names, dialogs and
  menus are announced as such, and controls like the mood meter use the
  matching roles.
- **Contrast and colour:** text meets 4.5:1 against its background (3:1 for
  large text) in the dark, light and both tinted themes, whichever accent
  colour you pick. Colour is never the only way something is shown: tags,
  file types and storage warnings also have text.
- **Motion:** animations follow your system's "reduce motion" setting.
- **Language:** the app is in English, Polish and German, with plural and
  grammatical-gender forms, and dates, numbers and units follow the region
  you choose.

## Contributor expectations

For changes to anything people see or use:

- `cd web && npm run check` must have zero warnings. It includes Svelte's
  accessibility checks. If you have to silence one, say why in the code.
- Use the colour tokens in `web/src/app.css`, never raw colours, so contrast
  holds in every theme. New text colours need 4.5:1 (3:1 for large text) in
  dark, light and both tinted themes. `fg-faint` is the faintest text colour
  that passes.
- Give icon-only buttons an `aria-label`. Use real buttons and links, not
  clickable `div`s. New full-screen overlays that close on Escape need
  `data-escape`.
- Use `lib/motion.js` for transitions, so "reduce motion" is respected.
- Translate new text into Polish and German (`web/tests/i18n.test.js` fails
  otherwise).
- In the pull request, say how you checked the change: which keys you used,
  which screen reader or browser accessibility tree you looked at, and which
  themes. A screenshot of a new screen in dark and light mode helps.

## Reporting accessibility issues

Open an issue with the **Accessibility barrier** form:
https://github.com/0x00ACAB/thencloud/issues/new?template=accessibility.yml

If you can't use GitHub, or would rather not post in public, use the private
report form (https://github.com/0x00ACAB/thencloud/security/advisories/new)
and say it's about accessibility.

What helps us most:

- what you were trying to do, and where (the page or dialog; don't paste a
  public link, the part after `#` is its key)
- what happened instead
- your browser or app and operating system, with versions if you know them
- any assistive technology you use (screen reader, magnifier, voice control,
  switch access) and its version

Screenshots or recordings are welcome but never required. You don't need to
tell us anything about a disability, and you don't need to know which
guideline is involved.

### Severity

We sort reports by how much they get in your way. You don't need to pick one;
we do that when we read the report.

- **Blocker:** you can't complete a task at all. For example: you can't sign
  in with a screen reader, or a dialog traps the keyboard.
- **Serious:** you can complete it, but only with real difficulty or someone
  else's help. For example: a button has no name, so you have to guess.
- **Moderate:** it's harder or slower than it should be. For example: focus
  jumps to the top of the list after closing a dialog.
- **Minor:** it's an annoyance that doesn't stop you. For example: an
  unclear label.

### How we respond

thencloud is maintained by a small group of volunteers, so these are aims,
not guarantees:

- We reply to every report within 7 days, to say we've seen it and what
  happens next.
- Blockers come before other work. If there's a way around the problem in the
  meantime, we tell you.
- We update the issue as we work on it, and ask you to try the fix before we
  close it, if you're willing.

## Ownership and maintenance

The project's maintainer (@0x00ACAB) is responsible for accessibility:
reviewing user-facing changes against the expectations above, triaging
reports, and keeping this page true. We review this page with every minor
release (1.1, 1.2...). If the maintainer changes, the new one takes this on
and says so here.

## Supported environments

What we build for and test with:

- **Browsers:** current versions of Chrome, Edge, Firefox and Safari. The
  automated browser tests and most hands-on checks use Chromium, so the
  others get less testing.
- **Apps:** the desktop app on Linux and Windows, and the Android app. They
  show the same web app in the system's web view.
- **Input:** mouse, touch and keyboard.
- **Assistive technology:** names, roles and focus have been checked in
  Chromium's accessibility tree, and contrast was measured in the browser in
  every theme. We haven't yet done a full pass with NVDA, JAWS, VoiceOver or
  TalkBack, or tested voice control or switch access. Reports from people
  who use them are especially welcome.

## Known limitations

- **Storage map:** in "What takes space", only folders and file types on the
  map can be reached with the keyboard. Individual files on it can't. The same
  sizes are listed as text under the map.
- **Charts in Health** are announced as a single image with a summary. Their
  individual points can only be explored with a pointer.
- **Videos** have captions only when you keep a subtitle file (`.srt` or
  `.vtt`) next to the video. Nothing is captioned automatically.
- **Office documents** are previewed as plain text, tables and pictures,
  without their layout. Images inside them are shown without descriptions,
  even when the document has them.
- **Scanned PDFs** have no text for a screen reader to read. PDFs with real
  text can be read and selected.
- **Turnstile:** if a server's admin turns on Cloudflare's bot check, sign-in
  shows Cloudflare's widget, whose accessibility we don't control. It's off
  by default.
- **Passkeys** don't work in the desktop and Android apps yet. Use a password
  and a one-time code there.
- **Translations:** only English, Polish and German so far.

## Feedback and improvements

If something here is wrong, missing or unclear, or you have an idea for doing
better, open an issue or a pull request that changes this file. For a barrier
that's stopping you right now, please use the reporting steps above instead,
so it gets triaged.
