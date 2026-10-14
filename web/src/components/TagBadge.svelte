<script>
  // A tag as shown on rows and in the tags dialog: the badge, with a dot in
  // the tag's colour when it has one. The text stays in the badge's own
  // colour, so it reads the same on any background.
  import { tagColour } from '../lib/places.svelte.js';

  let { tag, class: cls = '', children } = $props();

  const colour = $derived(tagColour(tag));
</script>

<span class="badge {cls}">
  {#if colour}<span class="tag-dot" style:background-color="var(--tag-{colour})" aria-hidden="true"></span>{/if}{tag}{@render children?.()}
</span>
