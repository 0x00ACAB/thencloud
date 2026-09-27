<script>
  // Dropdown menu anchored to a trigger button. `items` is a list of
  // { label, icon, onclick, danger } or 'sep' for a separator.
  import { tick } from 'svelte';
  import Icon from './Icon.svelte';
  import { pop, fade, portal } from '../lib/motion.js';

  let { items, label = 'More actions', trigger, align = 'end', buttonClass = 'btn btn-ghost btn-icon' } = $props();

  let open = $state(false);
  let btn = $state();
  let menu = $state();
  let top = $state(0);
  let left = $state(0);

  async function toggle() {
    if (open) return (open = false);
    open = true;
    await tick();
    const r = btn.getBoundingClientRect();
    const w = menu.offsetWidth;
    const h = menu.offsetHeight;
    left = Math.max(8, Math.min(align === 'end' ? r.right - w : r.left, innerWidth - w - 8));
    top = r.bottom + h + 8 > innerHeight ? Math.max(8, r.top - h - 4) : r.bottom + 4;
    menu.querySelector('button')?.focus({ preventScroll: true });
  }

  function choose(item) {
    open = false;
    item.onclick?.();
  }

  function onKey(e) {
    if (e.key === 'Escape') {
      open = false;
      btn.focus();
    } else if (e.key === 'ArrowDown' || e.key === 'ArrowUp') {
      e.preventDefault();
      const all = [...menu.querySelectorAll('button')];
      const i = all.indexOf(document.activeElement);
      all[(i + (e.key === 'ArrowDown' ? 1 : -1) + all.length) % all.length]?.focus({ preventScroll: true });
    }
  }

  $effect(() => {
    if (!open) return;
    const outside = (e) => !menu?.contains(e.target) && !btn?.contains(e.target) && (open = false);
    const close = () => (open = false);
    addEventListener('pointerdown', outside, true);
    addEventListener('scroll', close, true);
    addEventListener('resize', close);
    return () => {
      removeEventListener('pointerdown', outside, true);
      removeEventListener('scroll', close, true);
      removeEventListener('resize', close);
    };
  });
</script>

<button bind:this={btn} type="button" class={buttonClass} aria-label={label} aria-haspopup="menu" aria-expanded={open} onclick={toggle}>
  {#if trigger}{@render trigger()}{:else}<Icon name="ellipsis" />{/if}
</button>

{#if open}
  <div bind:this={menu} use:portal in:pop out:fade={{ duration: 90 }} class="menu origin-top" role="menu" tabindex="-1" style:top="{top}px" style:left="{left}px" onkeydown={onKey}>
    {#each items as item, i (i)}
      {#if item === 'sep'}
        <div class="menu-sep" role="separator"></div>
      {:else}
        <button type="button" role="menuitem" class="menu-item" class:danger={item.danger} onclick={() => choose(item)}>
          {#if item.icon}<Icon name={item.icon} />{/if}
          {item.label}
        </button>
      {/if}
    {/each}
  </div>
{/if}
