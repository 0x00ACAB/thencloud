# Tor onion service

Run thencloud behind a Tor onion service and the server never learns its users' IP addresses. [`deploy/onion/torrc`](https://github.com/0x00ACAB/thencloud/blob/main/deploy/onion/torrc) has the lines to add to Tor's configuration.

```sh
thencloud-server --bind 127.0.0.1:8080 --limit-by-address false
```

- Every visitor reaches the server from Tor's own address, so `--limit-by-address false` limits sign-in attempts per account (and link passwords per link) instead. Otherwise one person's wrong guesses would lock everyone out.
- Onion addresses are plain `http://`, and that's fine: Tor encrypts the connection end to end and the address authenticates the server. Tor Browser treats onion pages as secure contexts, which the web client needs.
- Passkeys made on an onion address work, if the browser offers them there.
- Tor Browser forgets site data when it closes, so "Keep me signed in" only lasts until then.
- Keep the server's port bound to 127.0.0.1. If it's reachable directly as well, that way in shows addresses again.
- The CLI knows nothing about Tor; run it under `torsocks`.
