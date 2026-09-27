<script>
  // A date as text ("3 hours ago" or "27 Sep 2026"), with the full date and
  // time on hover.
  import { formatWhen, formatDate, fullDate } from '../lib/format.js';

  /** `ms`: milliseconds since the epoch. `relative`: "3 hours ago" style. */
  let { ms, relative = false, prefix = '' } = $props();
</script>

<time datetime={new Date(ms).toISOString()} title={fullDate(ms)}>{prefix}{relative ? formatWhen(ms) : formatDate(ms)}</time>
