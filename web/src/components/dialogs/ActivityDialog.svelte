<script>
  // Who did what in a folder, newest first. The server keeps who, what kind
  // and which node; names are decrypted here for items we can still reach.
  import { onMount } from 'svelte';
  import Modal from '../Modal.svelte';
  import Icon from '../Icon.svelte';
  import Avatar from '../Avatar.svelte';
  import PersonName from '../PersonName.svelte';
  import Time from '../Time.svelte';
  import { activity, profileOf, session, folderLabel } from '../../lib/cloud.svelte.js';
  import { toastError } from '../../lib/ui.svelte.js';
  import { fly } from '../../lib/motion.js';
  import { t, slots } from '../../lib/i18n.svelte.js';

  // `onopen(event)`: show the item in its folder.
  let { entry, onopen, onclose } = $props();

  const PAGE = 100;
  let list = $state(null);
  let more = $state(false);
  let loading = $state(false);

  async function load() {
    loading = true;
    try {
      const page = await activity(entry, list?.at(-1)?.id ?? null);
      list = [...(list ?? []), ...page];
      more = page.length === PAGE;
      for (const a of new Set(page.map((e) => e.actor))) {
        if (!(a in genders)) {
          genders[a] = null;
          profileOf(a).then((p) => (genders[a] = p?.details?.gender ?? null), () => {});
        }
      }
    } catch (e) {
      toastError(e);
      list ??= [];
    } finally {
      loading = false;
    }
  }

  onMount(load);

  // The actor's grammatical gender, if they shared it: Polish past tenses
  // take it ("dodała", "dodał").
  let genders = $state({});

  // One sentence per kind, with {name} and {item} filled by components.
  function sentence(e) {
    const gender = genders[e.actor];
    if (e.actor === session.me?.username) {
      switch (e.kind) {
        case 'added': return t('You added {item}', { gender });
        case 'changed': return t('You uploaded a new version of {item}', { gender });
        case 'renamed': return t('You renamed {item}', { gender });
        case 'moved': return t('You moved {item}', { gender });
        case 'trashed': return t('You moved {item} to the trash', { gender });
        case 'restored': return t('You restored {item}', { gender });
      }
    } else {
      switch (e.kind) {
        case 'added': return t('{name} added {item}', { gender });
        case 'changed': return t('{name} uploaded a new version of {item}', { gender });
        case 'renamed': return t('{name} renamed {item}', { gender });
        case 'moved': return t('{name} moved {item}', { gender });
        case 'trashed': return t('{name} moved {item} to the trash', { gender });
        case 'restored': return t('{name} restored {item}', { gender });
      }
    }
    return `{name} ${e.kind} {item}`;
  }
</script>

<Modal title={t('Activity')} description={t('In {name} and the folders in it, over the last 90 days. The server records who did what and roughly when, not names.', { name: folderLabel(entry) })} {onclose} class="max-w-lg">
  {#if list === null}
    <div class="grid gap-3" aria-hidden="true">
      {#each [0, 1, 2] as i (i)}
        <div class="flex items-center gap-3"><div class="skeleton size-7 rounded-full"></div><div class="skeleton h-3.5 w-64"></div></div>
      {/each}
    </div>
  {:else if !list.length}
    <p class="text-[13px] text-fg-muted">{t('Nothing has happened here yet.')}</p>
  {:else}
    <ul class="-mx-1 grid max-h-96 gap-3 overflow-y-auto px-1" in:fly>
      {#each list as e (e.id)}
        <li class="flex items-start gap-3">
          <Avatar username={e.actor} class="mt-0.5 size-7 text-xs" />
          <div class="min-w-0 flex-1 text-sm">
            <p class="break-words">
              {#each slots(sentence(e)) as part, i (i)}
                {#if typeof part === 'string'}{part}{:else if part.slot === 'name'}<PersonName username={e.actor} class="font-medium" />{:else if e.name !== null}<button type="button" class="link font-medium" onclick={() => onopen(e)}>{e.name}</button>{:else}<span class="text-fg-muted">{e.folder ? t("a folder that's no longer here") : t("a file that's no longer here")}</span>{/if}
              {/each}
            </p>
            <!-- Times are kept to the hour, so the last hour is as close as it gets. -->
            <p class="text-xs text-fg-muted">{#if Date.now() - e.at * 1000 < 3600_000}{t('In the last hour')}{:else}<Time ms={e.at * 1000} relative />{/if}</p>
          </div>
        </li>
      {/each}
    </ul>
    {#if more}
      <button type="button" class="btn btn-ghost justify-self-start" disabled={loading} onclick={load}>
        {#if loading}<Icon name="loader-circle" class="spinner" />{/if} {t('Show older')}
      </button>
    {/if}
  {/if}
  {#snippet footer()}
    <button type="button" class="btn btn-secondary" onclick={onclose}>{t('Done')}</button>
  {/snippet}
</Modal>
