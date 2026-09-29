<script>
  // Someone's display name, if they gave us one, with their username after
  // it: a display name is theirs to pick, so it must never stand in for the
  // account (anyone could call themselves "alice").
  import { profileOf, session } from '../lib/cloud.svelte.js';
  import { t } from '../lib/i18n.svelte.js';

  let { username, you = false, class: cls = '' } = $props();

  let name = $state(null);
  $effect(() => {
    let live = true;
    name = null;
    if (username) profileOf(username).then((p) => live && (name = p.name));
    return () => (live = false);
  });
</script>

{#if you && username === session.me?.username}<span class={cls}>{t('You')}</span>{:else if name}<span class={cls}><span dir="auto">{name}</span> <span class="font-normal text-fg-muted">@{username}</span></span>{:else}<span class={cls}>{username}</span>{/if}
