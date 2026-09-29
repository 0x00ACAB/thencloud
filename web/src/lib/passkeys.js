// WebAuthn calls for passkeys. This only talks to the browser; the PRF
// output it returns is key material and goes straight to cloud.svelte.js,
// which turns it into a key and drops it.

import { inApp } from './server.svelte.js';

const bytes = (buf) => new Uint8Array(buf);
const prfFirst = (cred) => {
  const r = cred.getClientExtensionResults?.().prf;
  return { enabled: !!r?.enabled || !!r?.results, output: r?.results?.first ? bytes(r.results.first) : null };
};

// A passkey is tied to the server's host name, which the apps' pages don't have.
export const passkeysSupported = () => !inApp && typeof PublicKeyCredential !== 'undefined' && !!navigator.credentials?.create;

/** Make a passkey. `opts` comes from POST /api/passkeys/options (bytes already decoded). */
export async function createPasskey({ challenge, userHandle, exclude, username, prfSalt }) {
  const cred = await navigator.credentials.create({
    publicKey: {
      rp: { name: 'thencloud', id: location.hostname },
      user: { id: userHandle, name: username, displayName: username },
      challenge,
      pubKeyCredParams: [-8, -7, -257].map((alg) => ({ type: 'public-key', alg })),
      excludeCredentials: exclude.map((id) => ({ type: 'public-key', id })),
      authenticatorSelection: { residentKey: 'preferred', userVerification: 'preferred' },
      attestation: 'none',
      extensions: { prf: { eval: { first: prfSalt } } },
    },
  });
  const prf = prfFirst(cred);
  return {
    id: bytes(cred.rawId),
    clientDataJSON: bytes(cred.response.clientDataJSON),
    attestationObject: bytes(cred.response.attestationObject),
    prfEnabled: prf.enabled,
    prf: prf.output,
  };
}

/** Use a passkey. An empty `allow` lets the person pick any of theirs for this site. */
export async function usePasskey({ challenge, allow = [], prfSalt = null, verify = 'preferred' }) {
  const cred = await navigator.credentials.get({
    publicKey: {
      challenge,
      rpId: location.hostname,
      allowCredentials: allow.map((id) => ({ type: 'public-key', id })),
      userVerification: verify,
      ...(prfSalt ? { extensions: { prf: { eval: { first: prfSalt } } } } : {}),
    },
  });
  const r = cred.response;
  return {
    id: bytes(cred.rawId),
    clientDataJSON: bytes(r.clientDataJSON),
    authenticatorData: bytes(r.authenticatorData),
    signature: bytes(r.signature),
    userHandle: r.userHandle ? bytes(r.userHandle) : null,
    prf: prfFirst(cred).output,
  };
}

/** A cancelled or timed-out prompt, which isn't worth an error message. */
export const cancelled = (e) => e?.name === 'NotAllowedError' || e?.name === 'AbortError';
