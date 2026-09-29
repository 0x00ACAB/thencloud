// The Turnstile check some servers ask for before sign-in. It runs on /auth,
// a page of its own (AuthCheck.svelte), because Cloudflare's script must
// never share a page with the password or any key. That page leaves the
// token here, in this tab's sessionStorage, and comes back to /.
//
// A token is good once, for about five minutes. It's not a secret: it only
// says a person passed the check.

const TOKEN = 'turnstile';
const ARRIVED = 'turnstileArrived';
const RETURN = 'turnstileReturn';
// Cloudflare's tokens last 300 seconds; leave time for the request.
const LIFETIME = 270_000;

function read(key) {
  try {
    return sessionStorage.getItem(key);
  } catch {
    return null; // storage blocked
  }
}

function write(key, value) {
  try {
    if (value === null) sessionStorage.removeItem(key);
    else sessionStorage.setItem(key, value);
  } catch {
    /* storage blocked: the check just has to be done again */
  }
}

function stored() {
  try {
    const v = JSON.parse(read(TOKEN));
    return typeof v?.token === 'string' && Date.now() - v.at < LIFETIME ? v : null;
  } catch {
    return null;
  }
}

/** Whether a fresh token is waiting. */
export const hasToken = () => stored() !== null;

/** The token, used up: every request needs a new one. */
export function takeToken() {
  const v = stored();
  write(TOKEN, null);
  return v?.token ?? null;
}

/** On /auth: keep the token and go back to sign-in. */
export function returnWithToken(token) {
  write(TOKEN, JSON.stringify({ token, at: Date.now() }));
  write(ARRIVED, '1');
  location.replace('/');
}

/**
 * Go to /auth for the check. `state` (the tab, an invite) comes back with
 * `returned()`; the password never goes along.
 */
export function goToCheck(state) {
  write(RETURN, JSON.stringify(state));
  location.assign('/auth');
}

/** What `goToCheck` kept, once. */
export function returned() {
  const v = read(RETURN);
  write(RETURN, null);
  try {
    return v ? JSON.parse(v) : null;
  } catch {
    return null;
  }
}

/**
 * True once after coming back from /auth. Cloudflare's script ran on that
 * page with this origin's storage, so whatever sign-in was kept there is
 * dropped instead of trusted.
 */
export function arrivedFromCheck() {
  const v = read(ARRIVED) === '1';
  write(ARRIVED, null);
  return v;
}
