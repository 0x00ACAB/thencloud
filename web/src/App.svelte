<script>
  import { onMount } from 'svelte';
  import { session, resume } from './lib/cloud.svelte.js';
  import Auth from './components/Auth.svelte';
  import Shell from './components/Shell.svelte';
  import Toasts from './components/Toasts.svelte';
  import Icon from './components/Icon.svelte';

  // A browser the user chose to keep signed in skips the sign-in screen.
  let resuming = $state(true);
  onMount(() => {
    resume()
      .catch(() => {})
      .finally(() => (resuming = false));
  });
</script>

{#if session.me}
  <Shell />
{:else if resuming}
  <div class="grid min-h-dvh place-items-center text-fg-muted" aria-busy="true" aria-label="Unlocking your files">
    <Icon name="loader-circle" class="spinner size-5" />
  </div>
{:else}
  <Auth />
{/if}
<Toasts />
