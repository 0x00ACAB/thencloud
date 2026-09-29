<script>
  // /auth: the Turnstile check some servers ask for before sign-in. This is
  // the only page where Cloudflare's script may run (the server sends it a
  // CSP that allows challenges.cloudflare.com), so it has no password field
  // and loads no keys: it gets a token and goes back to /.
  import { onMount } from 'svelte';
  import Icon from './components/Icon.svelte';
  import { t } from './lib/i18n.svelte.js';
  import { hasRememberedSession } from './lib/remember.js';
  import { returnWithToken } from './lib/turnstile.js';

  const SCRIPT = 'https://challenges.cloudflare.com/turnstile/v0/api.js?render=explicit';

  let box = $state();
  let phase = $state('loading'); // loading | ready | failed
  let widget = null;

  function loadScript() {
    if (window.turnstile) return Promise.resolve();
    return new Promise((resolve, reject) => {
      const s = document.createElement('script');
      s.src = SCRIPT;
      s.async = true;
      s.onload = resolve;
      s.onerror = reject;
      document.head.appendChild(s);
    });
  }

  async function start() {
    phase = 'loading';
    // A kept sign-in stays out of reach of Cloudflare's script: with one,
    // there's nothing to check anyway.
    if (await hasRememberedSession()) return location.replace('/');
    let options = null;
    try {
      const res = await fetch('/api/auth/options', { cache: 'no-store' });
      options = res.ok ? await res.json() : null;
    } catch {
      /* offline */
    }
    if (!options) {
      phase = 'failed';
      return;
    }
    if (!options.turnstile) return location.replace('/');
    try {
      await loadScript();
    } catch {
      phase = 'failed';
      return;
    }
    phase = 'ready';
    if (widget !== null) window.turnstile.remove(widget);
    widget = window.turnstile.render(box, {
      sitekey: options.turnstile.site_key,
      action: 'auth',
      theme: document.documentElement.classList.contains('dark') ? 'dark' : 'light',
      language: document.documentElement.lang || 'auto',
      callback: (token) => returnWithToken(token),
      'error-callback': () => {
        phase = 'failed';
        return true;
      },
      'expired-callback': () => window.turnstile.reset(widget),
    });
  }

  onMount(() => {
    document.title = `${t('One moment')} · thencloud`;
    start();
  });
</script>

<main class="flex min-h-dvh flex-col items-center px-4 pt-[12vh] pb-8">
  <img src="/img/logo.webp" alt="thencloud" width="715" height="349" class="h-auto w-56 select-none" draggable="false" />

  <div class="mt-8 w-full max-w-sm">
    <h1 class="text-base font-semibold tracking-tight">{t('One moment')}</h1>
    <p class="mt-1 text-[13px] leading-5 text-fg-muted">
      {t("This server asks Cloudflare to check that you're a person before you sign in. Cloudflare sees this page, not your password or your files.")}
    </p>

    <div class="mt-6 min-h-16" bind:this={box} aria-live="polite">
      {#if phase === 'loading'}
        <p class="flex items-center gap-2 text-[13px] text-fg-muted"><Icon name="loader-circle" class="spinner" />{t('Loading the check')}</p>
      {/if}
    </div>

    {#if phase === 'failed'}
      <p class="flex items-start gap-2 text-[13px] text-danger" role="alert">
        <Icon name="circle-alert" class="mt-0.5 shrink-0" />
        {t("The check didn't load or didn't go through. If something blocks challenges.cloudflare.com, allow it for this page.")}
      </p>
      <button class="btn btn-secondary mt-4" onclick={start}><Icon name="refresh-cw" />{t('Try again')}</button>
    {/if}

    <a class="mt-8 flex w-fit items-center gap-1.5 text-[13px] text-fg-muted hover:text-fg" href="/"><Icon name="arrow-left" />{t('Back to sign in')}</a>
  </div>
</main>
