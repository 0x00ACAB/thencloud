<script>
  import { onMount } from 'svelte';
  import Modal from '../Modal.svelte';
  import Icon from '../Icon.svelte';
  import Avatar from '../Avatar.svelte';
  import { comments, addComment, deleteComment, session, MAX_COMMENT } from '../../lib/cloud.svelte.js';
  import { toastError } from '../../lib/ui.svelte.js';
  import { formatWhen, fullDate } from '../../lib/format.js';
  import { fly, slide } from '../../lib/motion.js';

  // `isOwner`: the node is ours, so any comment on it can be deleted.
  let { entry, isOwner, onclose } = $props();

  let list = $state(null);
  let text = $state('');
  let posting = $state(false);
  let busy = $state(null); // comment id being deleted
  let listEl = $state();

  onMount(async () => {
    try {
      list = await comments(entry);
    } catch (e) {
      toastError(e);
      list = [];
    }
  });

  async function post(e) {
    e?.preventDefault();
    const t = text.trim();
    if (!t || posting) return;
    posting = true;
    try {
      list = [...list, await addComment(entry, t)];
      text = '';
      requestAnimationFrame(() => listEl?.scrollTo({ top: listEl.scrollHeight }));
    } catch (err) {
      toastError(err);
    } finally {
      posting = false;
    }
  }

  async function remove(c) {
    busy = c.id;
    try {
      await deleteComment(c.id);
      list = list.filter((x) => x.id !== c.id);
    } catch (e) {
      toastError(e);
    } finally {
      busy = null;
    }
  }

  function keydown(e) {
    if (e.key === 'Enter' && (e.ctrlKey || e.metaKey)) post(e);
  }
</script>

<Modal
  title="Comments"
  description="On {entry.meta.name}. Encrypted like the file itself: everyone who can open it can read them, and the server can't."
  {onclose}
  onsubmit={post}
  class="max-w-lg">
  {#if list === null}
    <div class="grid gap-3" aria-hidden="true">
      {#each [0, 1] as i (i)}
        <div class="flex gap-3"><div class="skeleton size-7 rounded-full"></div><div class="grid flex-1 gap-1.5"><div class="skeleton h-3 w-24"></div><div class="skeleton h-3.5 w-56"></div></div></div>
      {/each}
    </div>
  {:else if list.length}
    <ul class="-mx-1 grid max-h-80 gap-4 overflow-y-auto px-1" bind:this={listEl} in:fly>
      {#each list as c (c.id)}
        <li class="group flex gap-3" out:slide>
          <Avatar username={c.author} class="mt-0.5 size-7 text-xs" />
          <div class="min-w-0 flex-1">
            <p class="flex items-baseline gap-2 text-[13px]">
              <span class="font-medium">{c.author === session.me.username ? 'You' : c.author}</span>
              <span class="text-xs text-fg-muted" title={fullDate(c.at)}>{formatWhen(c.at)}</span>
            </p>
            {#if c.text === null}
              <p class="text-[13px] text-fg-muted italic">This comment can't be decrypted. It may have been tampered with.</p>
            {:else}
              <p class="text-sm break-words whitespace-pre-wrap">{c.text}</p>
            {/if}
          </div>
          {#if isOwner || c.author_id === session.me.user_id}
            <button
              type="button"
              class="btn btn-ghost btn-icon opacity-0 group-hover:opacity-100 focus-visible:opacity-100"
              aria-label="Delete comment"
              title="Delete"
              disabled={busy !== null}
              onclick={() => remove(c)}>
              {#if busy === c.id}<Icon name="loader-circle" class="spinner" />{:else}<Icon name="trash-2" />{/if}
            </button>
          {/if}
        </li>
      {/each}
    </ul>
  {:else}
    <p class="text-[13px] text-fg-muted">No comments yet.</p>
  {/if}

  <div class="field">
    <label class="sr-only" for="comment-text">Add a comment</label>
    <textarea
      id="comment-text"
      class="input min-h-20 resize-y py-2"
      placeholder="Add a comment"
      maxlength={MAX_COMMENT}
      bind:value={text}
      onkeydown={keydown}></textarea>
  </div>

  {#snippet footer()}
    <span class="mr-auto text-xs text-fg-muted">Ctrl+Enter to post</span>
    <button type="button" class="btn btn-secondary" onclick={onclose}>Done</button>
    <button class="btn btn-primary" disabled={posting || !text.trim() || list === null}>
      {#if posting}<Icon name="loader-circle" class="spinner" />{/if}
      Post
    </button>
  {/snippet}
</Modal>
