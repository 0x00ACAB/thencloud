// New sign-in alerts: sessions that signed in since you last looked (not
// this one) are shown in a banner until you say it was you or sign them
// out. Which ones you've seen is kept in the encrypted "prefs" app data
// (`seenSessions`), so it follows the account and the server doesn't learn
// what you dismissed. The first time, every session there is counts as
// seen, so nobody gets an alert about all their old devices at once.
import { listSessions, revokeSession, loadAppData, saveAppData } from './cloud.svelte.js';

const MAX_SEEN = 500;
const CHECK_EVERY = 5 * 60_000;

export const signIns = $state({ alerts: [] });

let timer = null;
let checking = null;

export function check() {
  checking ??= (async () => {
    try {
      const [sessions, prefs] = await Promise.all([listSessions(), loadAppData('prefs')]);
      const ids = sessions.map((s) => s.id);
      if (!Array.isArray(prefs.seenSessions)) {
        await saveAppData('prefs', (d) => (d.seenSessions = ids));
        signIns.alerts = [];
        return;
      }
      const seen = new Set(prefs.seenSessions);
      signIns.alerts = sessions.filter((s) => !s.current && !seen.has(s.id)).sort((a, b) => b.created_at - a.created_at);
    } catch {
      // An older server, or offline: try again later.
    } finally {
      checking = null;
    }
  })();
  return checking;
}

/** Remember these sessions as seen; ids of sessions that are gone are dropped. */
async function markSeen(ids) {
  signIns.alerts = signIns.alerts.filter((s) => !ids.includes(s.id));
  const live = new Set((await listSessions().catch(() => [])).map((s) => s.id));
  await saveAppData('prefs', (d) => {
    const kept = (Array.isArray(d.seenSessions) ? d.seenSessions : []).filter((id) => live.size === 0 || live.has(id));
    d.seenSessions = [...new Set([...kept, ...ids])].slice(-MAX_SEEN);
  });
}

/** "It was me." */
export const dismiss = (s) => markSeen([s.id]);

/** "Sign it out." */
export async function signOut(s) {
  await revokeSession(s.id);
  await markSeen([s.id]);
}

/** Look now, then every few minutes and when the tab comes back into view. */
export function watchSignIns() {
  check();
  clearInterval(timer);
  timer = setInterval(check, CHECK_EVERY);
  const onVisible = () => document.visibilityState === 'visible' && check();
  document.addEventListener('visibilitychange', onVisible);
  return () => {
    clearInterval(timer);
    document.removeEventListener('visibilitychange', onVisible);
    signIns.alerts = [];
  };
}
