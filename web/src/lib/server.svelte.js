// Which server the client talks to. In the browser that's always the one
// that served the page. The desktop and Android apps (crates/thencloud-app)
// bundle this client instead, built with `--mode app`, and talk to a server
// the user picks; its address is kept in localStorage (it's not a secret).

export const inApp = import.meta.env.MODE === 'app';

const KEY = 'server';
const read = () => {
  try {
    return localStorage.getItem(KEY) || null;
  } catch {
    return null;
  }
};

const state = $state({ origin: inApp ? read() : location.origin });

/** The server's origin, e.g. https://cloud.example.com (null in the app before one is picked). */
export const serverOrigin = () => state.origin;

/** Where an API path goes. */
export const apiUrl = (path) => (inApp ? state.origin + path : path);

/**
 * The origin in what the user typed, or null. https only, except for this
 * device itself, so the token never crosses a network in the clear.
 */
export function parseServer(input) {
  let u;
  try {
    u = new URL(/^[a-z][\w+.-]*:\/\//i.test(input.trim()) ? input.trim() : `https://${input.trim()}`);
  } catch {
    return null;
  }
  const local = ['localhost', '127.0.0.1'].includes(u.hostname);
  if (u.protocol !== 'https:' && !(u.protocol === 'http:' && local)) return null;
  return u.origin;
}

export function setServer(origin) {
  state.origin = origin;
  try {
    if (origin) localStorage.setItem(KEY, origin);
    else localStorage.removeItem(KEY);
  } catch {
    /* kept for this run only */
  }
}
