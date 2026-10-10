# Threat model

thencloud assumes the server may be curious or compromised, and tries to make sure it learns as little as possible and can't tamper without being caught.

## What the server can see

- Usernames and public keys.
- The shape of the folder tree: which node is inside which, and file or folder.
- Whether two items in the same folder have the same name (each carries a keyed hash of its lower-cased name under the folder key, so duplicates can be refused), but not what the names are.
- Ciphertext sizes and chunk counts. Padding makes these rough, not hidden.
- Timestamps, rounded to the hour. The exact times are inside the encrypted metadata.
- Who shares with whom, with what permission, and which nodes have public links.
- IP addresses and access patterns (unless it's an [onion service](../host/tor.md)).

## What the server can't see

- Passwords, master keys or private keys.
- File and folder keys.
- File and folder names, MIME types, plaintext sizes and modification times.
- File contents.

## No analytics

thencloud has no telemetry, tracking or crash reporting, and never will. The web client talks only to your server: its Content Security Policy allows nothing else (no CDNs, fonts or third-party scripts), a server test fails if that policy ever lets another host in, and the browser tests fail if any request goes elsewhere. The server makes no connections of its own, except to an S3 bucket you configure, to Cloudflare if you turn on the [bot check](../host/turnstile.md), and to video sites if an admin turns on the [downloader](../host/downloader.md).

## Known limitations

- **The web client is served by the server.** A malicious server could serve modified JavaScript. This is true of every browser-based end-to-end encrypted app. The [apps](../clients/apps.md) and the [CLI](../clients/cli.md) avoid it, and releases make it checkable with [`verify-web`](verify.md). Inside the browser, the service worker remembers the hash of each page and code file and warns when they change, but the browser fetches the service worker itself from the server, so a server that replaces it first can get past it.
- **Public keys are trust-on-first-use.** Compare fingerprints out of band the first time you share with someone. After that the key is pinned, and a different key blocks sharing until you check again.
- **Removing a share** stops the server serving the data, but doesn't re-key. A former recipient who kept the key could decrypt ciphertext they get elsewhere.
- **File drop links** carry your public key. Anyone with the link, the server included, can add files to that folder, but only you can read them.
- **Anyone with a full public link,** including the `#` part (for example from chat history), can decrypt what it points to.
- **The video downloader** sees the links and videos it fetches. It's off unless an admin turns it on.
- **"Keep me signed in"** keeps your session and master key in the browser, encrypted with a key no script can export. That's about as safe as your browser profile and disk encryption: anyone who can use that computer account can open your files.
- **There's no password reset.** Without a recovery key, a forgotten password means the data is lost.
- **Previews render files other people shared with you** inside the app, where your keys live. Decrypted bytes always get a fixed, known-safe type, Markdown is sanitised and never loads images, Office documents are shown as plain data, and PDFs are drawn to a canvas without running their JavaScript. The CSP is the second line of defence.
- **No independent security audit yet.**
