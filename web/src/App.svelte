<script>
  import { session } from './lib/cloud.svelte.js';
  import Auth from './components/Auth.svelte';
  import Shell from './components/Shell.svelte';
  import Toasts from './components/Toasts.svelte';
</script>

{#if session.me}
  <Shell />
{:else}
  <Auth />
{/if}
<Toasts />
