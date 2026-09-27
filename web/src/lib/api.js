// Thin fetch wrapper for the thencloud JSON API.

export class ApiError extends Error {
  constructor(status, body) {
    super(body?.message || `Request failed (HTTP ${status})`);
    this.status = status;
    this.code = body?.error;
  }
}

/**
 * `body` is sent as JSON, `raw` as bytes. Resolves to parsed JSON, a
 * Uint8Array for binary responses, or null for 204.
 */
export async function request(method, path, { token, body, raw, headers } = {}) {
  const h = { ...headers };
  if (token) h.Authorization = `Bearer ${token}`;
  let payload;
  if (raw !== undefined) {
    payload = raw;
    h['Content-Type'] = 'application/octet-stream';
  } else if (body !== undefined) {
    payload = JSON.stringify(body);
    h['Content-Type'] = 'application/json';
  }
  let res;
  try {
    res = await fetch(path, { method, headers: h, body: payload, cache: 'no-store', referrerPolicy: 'no-referrer' });
  } catch {
    throw new ApiError(0, { message: 'Could not reach the server. Check your connection.' });
  }
  if (!res.ok) {
    let b = null;
    try {
      b = await res.json();
    } catch {
      /* not json */
    }
    throw new ApiError(res.status, b);
  }
  if (res.status === 204) return null;
  if ((res.headers.get('content-type') || '').includes('application/json')) return res.json();
  return new Uint8Array(await res.arrayBuffer());
}
