# Behind Cloudflare

thencloud works behind Cloudflare's proxy or a Cloudflare Tunnel, but two of Cloudflare's features have to be off for its address, because they put Cloudflare's own JavaScript into its pages:

- **Rocket Loader** rewrites every `<script>` tag and adds a loader script.
- **JavaScript detections**, part of Bot Fight Mode, adds a hidden frame that loads a challenge script.

Both are served from your own domain (under `/cdn-cgi/`), so thencloud's Content-Security-Policy, which only allows scripts from its own origin, lets them in. That's Cloudflare code running in the page that holds your keys and the link key after `#`, which thencloud otherwise never allows. You'd also see `thencloud verify-web` fail on the HTML pages, and the app warn that the page it was sent has changed.

## Turning them off for thencloud only

On the Free plan, JavaScript detections can't be turned off on their own or skipped per hostname, but a **configuration rule** stops Cloudflare changing thencloud's HTML at all:

1. In the Cloudflare dashboard, go to **Rules > Configuration Rules** and create a rule.
2. Match **Hostname equals** your thencloud address, for example `cloud.example.com`.
3. Set **Rocket Loader** to off, and **Response Body Buffering** to **None**.
4. Deploy it.

With buffering set to None, Cloudflare passes the page through as the server sent it, so neither feature can add to it. Bot Fight Mode itself keeps running, and your other sites keep their settings.

Through the API, the same rule (in the `http_config_settings` phase of the zone):

```json
{
  "expression": "(http.host eq \"cloud.example.com\")",
  "action": "set_config",
  "action_parameters": { "rocket_loader": false, "response_body_buffering": "none" }
}
```

## Checking

```sh
curl -s https://cloud.example.com/ | grep -c 'cdn-cgi'      # expect 0
thencloud verify-web https://cloud.example.com --manifest thencloud-web-<version>.json
```

`verify-web` should report that every response matches the release.

## Other settings

- **Email obfuscation**, **Auto Minify** (where still offered) and **Web Analytics** also change HTML; buffering set to None stops them too.
- Cache rules: leave thencloud's own `Cache-Control` headers in charge. Pages are `no-cache`, the API is `no-store` and only the hashed `/assets/` are cached for good.
- Turn on `--trust-proxy` only when Cloudflare is the only way in (a Tunnel, or a firewall that allows only Cloudflare's addresses), so sign-in limits use the visitor's address rather than Cloudflare's.
