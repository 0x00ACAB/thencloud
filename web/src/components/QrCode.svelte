<script>
  // A QR code as one SVG path, drawn dark on light whatever the theme so
  // phone cameras read it. The encoder is loaded only when one is shown.
  let { text, label, class: cls = '' } = $props();

  let qr = $state(null);
  $effect(() => {
    const t = text;
    import('uqr').then(({ encode }) => (qr = encode(t, { ecc: 'M', border: 2 })));
  });

  const path = $derived.by(() => {
    if (!qr) return '';
    let d = '';
    qr.data.forEach((row, y) => row.forEach((dark, x) => dark && (d += `M${x} ${y}h1v1h-1z`)));
    return d;
  });
</script>

{#if qr}
  <svg viewBox="0 0 {qr.size} {qr.size}" class={cls} role="img" aria-label={label} shape-rendering="crispEdges">
    <rect width={qr.size} height={qr.size} fill="#fff" />
    <path d={path} fill="#000" />
  </svg>
{:else}
  <div class="skeleton {cls}" aria-hidden="true"></div>
{/if}
