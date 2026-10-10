# Bot check (Turnstile)

A server can ask for a [Cloudflare Turnstile](https://www.cloudflare.com/products/turnstile/) check before password sign-in, and before registration while it's open to everyone. It's off by default.

```sh
thencloud-server --turnstile-site-key <site key> --turnstile-secret <secret>
```

Create the widget in Cloudflare's dashboard for your server's host name. If people reach the server under more than one name, list them with `--turnstile-hostnames`.

## How it's kept away from your keys

This is the one place the web client loads a script from someone else, so it gets a page of its own, `/auth`:

- Only `/auth`'s Content Security Policy lets Cloudflare's script and frame in. The app page's policy doesn't change.
- `/auth` has no password field, loads no WASM and reads no key. It hands the app page a one-time token and sends you back.
- A sign-in kept on the browser ("Keep me signed in") is dropped after a visit to `/auth`, since Cloudflare's script shared the site's storage there.
- The server checks the token with Cloudflare, sending the secret and the token and nothing else (not the username or address), and takes each token only once.

The first account, invited people, recovery keys, passkeys and app passwords skip the check. The desktop and Android apps can't show it yet, so their users can't sign in with a password while it's on.

`verify-web` accepts Cloudflare in the CSP of `/auth` only.
