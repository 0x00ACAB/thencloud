# Desktop and Android apps

The apps are the web client in a [Tauri](https://tauri.app) window, for Linux, Windows and Android. Download them from the [releases page](https://github.com/0x00ACAB/thencloud/releases).

The main reason to use one: **the app carries its own copy of the web client.** In a browser, the server sends the page's code every time, so a compromised server could send you different code. The app only talks to the server's API.

The first screen asks for your server's address (https; plain http only for `localhost`).

## What's different from the browser

- **No passkeys.** A passkey belongs to the server's host name, and the app's pages don't have one. Sign in with your password and, if you use one, a code from your authenticator app.
- **No bot check.** If the server asks for a [Turnstile check](../host/turnstile.md) before sign-in, the app can't show it yet.
- **Links open in your browser.** Public links and invites still point at the server, since that's where other people open them.
- **Almost no native access.** On Linux and Windows the page gets no Tauri permissions; the app only opens outside links and saves downloads. On Android it may also save a download to Downloads and show what music is playing.

## Android

- Downloads go to your Downloads folder, written as they're decrypted, without a storage permission.
- Music keeps playing with the screen off or another app open, with a notification, lock screen controls and headphone buttons. The track's name and cover go to Android on this device only.
- The back gesture closes what's on top first (a menu, a dialog, a preview), then goes back through where you've been.

## Building it yourself

See [`crates/thencloud-app/README.md`](https://github.com/0x00ACAB/thencloud/blob/main/crates/thencloud-app/README.md).
