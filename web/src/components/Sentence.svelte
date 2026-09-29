<script>
  // A translated sentence with components in it, in the translation's own
  // word order: <Sentence text={t('Checked {when}')}>{#snippet when()}<Time ... />{/snippet}</Sentence>.
  // Each {placeholder} left in the text is rendered by the snippet of that name.
  import { slots } from '../lib/i18n.svelte.js';

  let { text, ...fill } = $props();
</script>

{#each slots(text) as part, i (i)}{#if typeof part === 'string'}{part}{:else}{@render fill[part.slot]?.()}{/if}{/each}
