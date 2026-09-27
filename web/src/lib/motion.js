// Svelte transitions that respect the user's "reduce motion" setting, with
// the app's standard timings. Motion here is meant to be felt more than
// seen: short, small distances, no bounce.

import * as t from 'svelte/transition';
import { cubicOut } from 'svelte/easing';

const reduce = matchMedia('(prefers-reduced-motion: reduce)');

const tuned = (fn, defaults) => (node, params = {}) =>
  fn(node, reduce.matches ? { ...defaults, ...params, duration: 0, delay: 0 } : { easing: cubicOut, ...defaults, ...params });

/** Fade in place. */
export const fade = tuned(t.fade, { duration: 140 });
/** Fade plus a short slide (default: up from 4px below). */
export const fly = tuned(t.fly, { duration: 180, y: 4 });
/** Fade plus a slight scale, for popovers. */
export const pop = tuned(t.scale, { duration: 130, start: 0.97 });
/** Collapse/expand height, for list items leaving or entering. */
export const slide = tuned(t.slide, { duration: 160 });

export { flip } from 'svelte/animate';
export const flipParams = () => ({ duration: reduce.matches ? 0 : 180, easing: cubicOut });

/**
 * Svelte action: render the element at the end of <body>,
 * so no transformed or overflow-clipped ancestor can trap a fixed popover.
 */
export function portal(node) {
  document.body.append(node);
  return { destroy: () => node.remove() };
}
