// Storage alerts for admins (`GET /api/admin/alerts`): disks or a bucket
// nearly full, and accounts near their quota. Loaded when an admin signs in,
// so the sidebar can mark Admin, and again whenever the admin view changes
// something.
import { adminAlerts } from './cloud.svelte.js';

export const alerts = $state({ data: null });

export async function loadAlerts() {
  try {
    alerts.data = await adminAlerts();
  } catch {
    // Older servers don't have the route; nothing to show then.
    alerts.data = null;
  }
}

export const needsAttention = () => !!alerts.data && (alerts.data.spaces.some((s) => s.low) || alerts.data.near_quota.length > 0);

export function dropAlerts() {
  alerts.data = null;
}
