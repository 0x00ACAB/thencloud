<script>
  import { ICONS } from '../lib/icons.js';

  let { name, class: cls = 'size-4', strokeWidth = 2 } = $props();
</script>

<svg
  xmlns="http://www.w3.org/2000/svg"
  viewBox="0 0 24 24"
  fill="none"
  stroke="currentColor"
  stroke-width={strokeWidth}
  stroke-linecap="round"
  stroke-linejoin="round"
  class={cls}
  aria-hidden="true">{@html ICONS[name]}</svg>
