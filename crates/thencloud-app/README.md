# thencloud app

The web client in a [Tauri](https://tauri.app) window, for Linux, Windows and Android.

The app carries its own copy of the web client, built with `npm run build:app`, and only talks to your server's API. A server can't send it different code the way it could send a browser a different page. The first screen asks for the server's address (https, or http for `localhost`), which is kept on the device.

What differs from the browser:

- **No passkeys.** A passkey belongs to the server's host name, and the app's pages don't have one. Sign in with your password and, if you use one, a code from your authenticator app.
- **Links open in your browser.** Public links and invite links still point at the server, since that's where other people open them.
- **The page can reach almost nothing native.** On Linux and Windows it gets no Tauri permissions; the Rust side only opens outside links and saves downloads to your Downloads folder. On Android it may do two things: save a file you download, and show what music is playing (see below).

The server lets the app in with CORS for its origins only (`tauri://localhost`, `http(s)://tauri.localhost`); sessions are bearer tokens, so there are no cookies for another origin to ride on.

## Android

Android's WebView needs a few things done for it, which `android-plugin/` (a small Tauri plugin, Kotlin in `android/`) does:

- **Downloads.** The WebView drops a download of a `blob:` URL, which is how the page hands over a decrypted file, and it has no service worker to stream through. So the page passes each decrypted piece to the plugin (`web/src/lib/native.js`), which writes it to Downloads through MediaStore, with no storage permission (Android 9 and older ask where to save it instead). Large files are written as they're decrypted, not built in memory first. `capabilities/android.json` allows the plugin's three save commands, and the three below, on Android only.
- **The system bars and the keyboard.** The app draws edge to edge (Android 15 requires it), so the plugin tells the page where the status bar, navigation bar and cutouts are (`--android-inset-*`, used through `--safe-*` in `app.css`), shrinks the WebView while the keyboard is up, and marks that with a `keyboard-open` class so the tab bar steps aside.
- **Streaming.** wry serves the app's pages from the WebView client's request handler, which a service worker's requests skip, so `/sw.js` couldn't load. The plugin routes them through the same handler, so video, music and big previews stream (`web/src/lib/stream.js`) instead of being decrypted into memory whole. The worker doesn't watch the app's pages for changes (the "Pins" in `sw.js`): they come from the app, not a server.
- **Music in the background.** The WebView shows nothing for `navigator.mediaSession`, and Android stops an app in the background that has no foreground service. So the Music player tells the plugin what's playing (`web/src/lib/nowplaying.svelte.js`: title, artist, album, a cover it draws again as a small JPEG, the position), and the plugin shows it through a MediaSession (`NowPlaying.kt`): a notification, the lock screen, and Bluetooth and headphone buttons, which come back to the player's own handlers through `window.thencloudMedia`. While it plays, `PlaybackService.kt` runs as a `mediaPlayback` foreground service, so the music goes on with the screen off or another app open; paused for half a minute, it stops being one (the notification stays, and can be swiped away). The wait covers the moment between tracks, since a service that has stopped being a foreground one can't become one again from the background. Unplugging headphones pauses. Names and covers go to the system on this device only, and show on the lock screen like any music app's.
- **Back.** The back gesture first closes what's on top (a menu, a dialog, a preview, the selection), then goes back through the sections and folders you visited, and only then leaves the app.

### Trying it in an emulator

With the SDK's `emulator` and a system image (`sdkmanager "emulator" "system-images;android-36;google_apis;x86_64"`):

```sh
avdmanager create avd -n thencloud -k "system-images;android-36;google_apis;x86_64" -d pixel_7
emulator -avd thencloud &
cargo run -p thencloud-server -- --bind 127.0.0.1:8080 --data-dir ./data
adb reverse tcp:8080 tcp:8080             # the emulator's localhost:8080 is now this machine's
scripts/build-app.sh android --debug --apk --target x86_64
adb install -r crates/thencloud-app/gen/android/app/build/outputs/apk/universal/debug/app-universal-debug.apk
```

Then connect to `http://localhost:8080` (debug builds allow plain http; release builds don't). A debug build's WebView can be inspected from `chrome://inspect`, or driven with Playwright's `_android`.

## Building

You need what `build.sh` needs (rustup with `wasm32-unknown-unknown`, wasm-pack, Node.js), the [Tauri prerequisites](https://tauri.app/start/prerequisites/) for your system, and the Tauri CLI. For Android: the SDK and NDK (`ANDROID_HOME`, `NDK_HOME`), JDK 17 or 21 (Gradle doesn't run on 25 yet), and `rustup target add aarch64-linux-android armv7-linux-androideabi i686-linux-android x86_64-linux-android`.

```sh
cargo install tauri-cli --version "^2" --locked
```

Then, from the repository root:

```sh
scripts/build-app.sh            # Linux: .deb, .rpm and AppImage; Windows: NSIS and MSI installers
scripts/build-app.sh android    # an APK and an AAB (needs the Android SDK and NDK)
```

The results are in `crates/thencloud-app/target/release/bundle/` (Android: `gen/android/app/build/outputs/`).

The Android project in `gen/android` isn't kept in the repository; the script makes it with `cargo tauri android init` the first time. Release APKs need signing, see [Tauri's guide](https://tauri.app/distribute/sign/android/).

## Development

```sh
cd web && npm run dev:app                 # Vite with hot reload, in app mode
cd crates/thencloud-app && cargo tauri dev
```

Point the app at a server started with `cargo run -p thencloud-server -- --bind 127.0.0.1:8080` by entering `http://localhost:8080` (only loopback addresses may skip https). For Android, run `npm run dev:app -- --host` and `cargo tauri android dev`.

The icons in `icons/` are made from `web/public/icons/icon-512.png` with `cargo tauri icon`.
