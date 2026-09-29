<script>
  // What changed between two versions of a text file: both are decrypted
  // here and compared line by line (lib/diff.js), with a few unchanged
  // lines around each change.
  import { onMount } from 'svelte';
  import Modal from '../Modal.svelte';
  import Icon from '../Icon.svelte';
  import Time from '../Time.svelte';
  import Sentence from '../Sentence.svelte';
  import { fetchVersion } from '../../lib/cloud.svelte.js';
  import { readText, MAX_TEXT } from '../../lib/preview.js';
  import { diffLines, hunks, splitLines } from '../../lib/diff.js';
  import { formatSize } from '../../lib/format.js';
  import { errorMessage } from '../../lib/ui.svelte.js';
  import { t } from '../../lib/i18n.svelte.js';

  /** `before` and `after`: versions of `entry` (older first); `at(v)` is when each was made. */
  let { entry, before, after, at, onclose } = $props();

  let state = $state({ status: 'loading' }); // loading | ready | error
  const counts = $derived(
    state.status === 'ready' ? { added: state.ops.filter((o) => o.op === '+').length, removed: state.ops.filter((o) => o.op === '-').length } : null,
  );

  onMount(async () => {
    try {
      if (Math.max(before.meta.size, after.meta.size) > MAX_TEXT) {
        state = { status: 'error', message: t('These versions are too large to compare here (over {size}).', { size: formatSize(MAX_TEXT) }) };
        return;
      }
      const [a, b] = await Promise.all([fetchVersion(entry, before, () => {}), fetchVersion(entry, after, () => {})]);
      const [textA, textB] = await Promise.all([readText(a.blob), readText(b.blob)]);
      if (textA === null || textB === null) {
        state = { status: 'error', message: t("One of these versions isn't text, so there are no lines to compare.") };
        return;
      }
      const ops = diffLines(splitLines(textA), splitLines(textB));
      state = ops ? { status: 'ready', ops, hunks: hunks(ops) } : { status: 'error', message: t('These versions are too different to compare line by line.') };
    } catch (e) {
      state = { status: 'error', message: errorMessage(e) };
    }
  });

  const mark = { '+': '+', '-': '-', '=': ' ' };
</script>

<Modal title={t('Changes in {name}', { name: entry.meta.name })} {onclose} class="max-w-4xl">
  <p class="-mt-2 text-[13px] text-fg-muted">
    <Sentence text={t('From the version of {from} to the version of {to}')}>
      {#snippet from()}<Time ms={at(before)} relative />{/snippet}
      {#snippet to()}<Time ms={at(after)} relative />{/snippet}
    </Sentence>{#if counts}{' · '}<span class="tabular-nums">{t('{count} lines added', { count: counts.added })}, {t('{count} lines removed', { count: counts.removed })}</span>{/if}
  </p>
  {#if state.status === 'loading'}
    <div class="grid h-40 place-items-center text-fg-muted" aria-busy="true"><Icon name="loader-circle" class="spinner" /></div>
  {:else if state.status === 'error'}
    <p class="flex items-center gap-2 text-[13px] text-fg-muted"><Icon name="circle-alert" class="size-4 shrink-0" />{state.message}</p>
  {:else if !state.hunks.length}
    <p class="text-[13px] text-fg-muted">{t('The text is the same in both.')}</p>
  {:else}
    <div class="max-h-[60vh] overflow-auto rounded-md border border-line font-mono text-xs leading-5">
      {#each state.hunks as h, i (i)}
        {#if h.skippedBefore}
          <p class="border-y border-line bg-subtle px-3 py-0.5 text-fg-muted first:border-t-0">{t('{count} unchanged lines', { count: h.skippedBefore })}</p>
        {/if}
        <table class="w-full border-collapse">
          <tbody>
            {#each h.lines as l, j (j)}
              <tr class={l.op === '+' ? 'bg-diff-add' : l.op === '-' ? 'bg-diff-del' : ''}>
                <td class="w-12 px-2 text-right text-fg-muted tabular-nums select-none">{l.a !== undefined ? l.a + 1 : ''}</td>
                <td class="w-12 px-2 text-right text-fg-muted tabular-nums select-none">{l.b !== undefined ? l.b + 1 : ''}</td>
                <td class="w-4 text-fg select-none" aria-label={l.op === '+' ? t('Added') : l.op === '-' ? t('Removed') : undefined}>{mark[l.op]}</td>
                <td class="pr-3 break-all whitespace-pre-wrap">{l.text}</td>
              </tr>
            {/each}
          </tbody>
        </table>
      {/each}
    </div>
  {/if}
  {#snippet footer()}
    <button type="button" class="btn btn-secondary" onclick={onclose}>{t('Done')}</button>
  {/snippet}
</Modal>
