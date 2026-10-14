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

### Installing and updating through F-Droid

So your phone tells you about updates, add thencloud's own F-Droid repo to [F-Droid](https://f-droid.org), [Neo Store](https://github.com/NeoApplications/Neo-Store) or [Obtainium](https://github.com/ImranR98/Obtainium). On your phone, open this link:

[`https://docs.kkaii.xyz/fdroid/repo?fingerprint=70A55BA6378D8CDF5754CE11AD0AF39A169FDAB37718D649EE1D415BC627F25B`](https://docs.kkaii.xyz/fdroid/repo?fingerprint=70A55BA6378D8CDF5754CE11AD0AF39A169FDAB37718D649EE1D415BC627F25B)

Or add it by hand, in F-Droid and Neo Store under Settings > Repositories:

- Address: `https://docs.kkaii.xyz/fdroid/repo`
- Fingerprint: `70A55BA6378D8CDF5754CE11AD0AF39A169FDAB37718D649EE1D415BC627F25B`

In Obtainium, choose **Add app**, paste the address with the fingerprint, and pick thencloud. (Obtainium can also follow the GitHub releases directly: add `https://github.com/0x00ACAB/thencloud`.)

The repo lists the APKs of the latest three releases, the same files that are on the releases page. Each one is signed with thencloud's release key (SHA-256 `a4f82864f771a858f3e01788262850a14fd725c848deb565e694a775cb9f5aff`). The repo's list is signed with a key of its own, which is what the fingerprint pins: your app store refuses a list someone else changed. If you already installed the APK from the releases page, the repo's updates install over it.

## Building it yourself

See [`crates/thencloud-app/README.md`](https://github.com/0x00ACAB/thencloud/blob/main/crates/thencloud-app/README.md).
