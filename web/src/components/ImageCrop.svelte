<script>
  import { untrack } from 'svelte';

  // A crop box over an image: drag inside to move it, drag a corner to
  // resize. `crop` is in the image's own pixels: { x, y, w, h }.
  let { src, width, height, crop = $bindable(), aspect = null } = $props();

  let shown = $state(0); // displayed width of the image, in CSS pixels
  const scale = $derived(shown ? shown / width : 1);
  const MIN = 8;

  const clamp = (v, lo, hi) => Math.min(hi, Math.max(lo, v));

  /** Keep `c` inside the image, at least MIN pixels, and at `aspect` if set. */
  function fit(c, anchor) {
    let { x, y, w, h } = c;
    w = clamp(w, MIN, width);
    h = clamp(h, MIN, height);
    if (aspect) {
      h = w / aspect;
      if (h > height) (h = height), (w = h * aspect);
    }
    if (anchor?.includes('w')) x = c.x + c.w - w;
    if (anchor?.includes('n')) y = c.y + c.h - h;
    x = clamp(x, 0, width - w);
    y = clamp(y, 0, height - h);
    return { x: Math.round(x), y: Math.round(y), w: Math.round(w), h: Math.round(h) };
  }

  // When the aspect preset changes, reshape around the current centre.
  $effect(() => {
    if (!aspect) return;
    untrack(() => {
      if (!crop) return;
      const cx = crop.x + crop.w / 2;
      const cy = crop.y + crop.h / 2;
      let w = crop.w;
      let h = w / aspect;
      if (h > height) (h = height), (w = h * aspect);
      if (w > width) (w = width), (h = w / aspect);
      crop = fit({ x: cx - w / 2, y: cy - h / 2, w, h });
    });
  });

  let drag = null; // { mode, startX, startY, start }

  function down(e, mode) {
    e.preventDefault();
    e.stopPropagation();
    e.currentTarget.setPointerCapture(e.pointerId);
    drag = { mode, startX: e.clientX, startY: e.clientY, start: { ...crop } };
  }

  function move(e) {
    if (!drag) return;
    const dx = (e.clientX - drag.startX) / scale;
    const dy = (e.clientY - drag.startY) / scale;
    const s = drag.start;
    if (drag.mode === 'move') {
      crop = { ...s, x: Math.round(clamp(s.x + dx, 0, width - s.w)), y: Math.round(clamp(s.y + dy, 0, height - s.h)) };
      return;
    }
    let { x, y, w, h } = s;
    if (drag.mode.includes('e')) w = clamp(s.w + dx, MIN, width - s.x);
    if (drag.mode.includes('s')) h = clamp(s.h + dy, MIN, height - s.y);
    if (drag.mode.includes('w')) w = clamp(s.w - dx, MIN, s.x + s.w);
    if (drag.mode.includes('n')) h = clamp(s.h - dy, MIN, s.y + s.h);
    crop = fit({ x, y, w, h }, drag.mode);
  }

  const up = () => (drag = null);

  // Arrow keys nudge the box (Shift for 10px), for keyboard users.
  function key(e) {
    const step = e.shiftKey ? 10 : 1;
    const d = { ArrowLeft: [-step, 0], ArrowRight: [step, 0], ArrowUp: [0, -step], ArrowDown: [0, step] }[e.key];
    if (!d) return;
    e.preventDefault();
    crop = { ...crop, x: clamp(crop.x + d[0], 0, width - crop.w), y: clamp(crop.y + d[1], 0, height - crop.h) };
  }
</script>

<div class="relative mx-auto w-fit touch-none overflow-hidden rounded border border-line select-none" role="group" aria-label="Image to crop" onpointermove={move} onpointerup={up} onpointercancel={up}>
  <img {src} alt="" class="checkerboard block max-h-64 max-w-full" bind:clientWidth={shown} draggable="false" />
  {#if crop && shown}
    <div
      class="crop-box absolute cursor-move"
      style:left="{crop.x * scale}px"
      style:top="{crop.y * scale}px"
      style:width="{crop.w * scale}px"
      style:height="{crop.h * scale}px"
      role="slider"
      tabindex="0"
      aria-label="Crop area, {crop.w} by {crop.h} pixels. Arrow keys move it."
      aria-valuenow={crop.x}
      onpointerdown={(e) => down(e, 'move')}
      onkeydown={key}>
      {#each ['nw', 'ne', 'sw', 'se'] as corner (corner)}
        <span class="crop-handle crop-{corner}" onpointerdown={(e) => down(e, corner)} aria-hidden="true"></span>
      {/each}
    </div>
  {/if}
</div>
