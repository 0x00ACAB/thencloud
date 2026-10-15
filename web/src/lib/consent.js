// The consent page a linking popup may be sent to. The address comes from
// the server, and in the desktop and Android apps the page's code doesn't,
// so a server can't use the popup to open a page of its own choosing:
// only Google's sign-in, or the stand-in Google on the loopback address
// the browser tests run.

const LOOPBACK = new Set(['127.0.0.1', 'localhost', '[::1]']);

/** Whether `url` is a consent page the linking popup may open. */
export function consentUrlAllowed(url) {
  let u;
  try {
    u = new URL(url);
  } catch {
    return false;
  }
  if (u.username || u.password) return false;
  if (u.protocol === 'https:') return u.host === 'accounts.google.com';
  return u.protocol === 'http:' && LOOPBACK.has(u.hostname);
}
