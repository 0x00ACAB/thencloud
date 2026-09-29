# thencloud app

The web client in a [Tauri](https://tauri.app) window, for Linux, Windows and Android.

The app carries its own copy of the web client, built with `npm run build:app`, and only talks to your server's API. A server can't send it different code the way it could send a browser a different page. The first screen asks for the server's address (https, or http for `localhost`), which is kept on the device.

What differs from the browser:

- **No passkeys.** A passkey belongs to the server's host name, and the app's pages don't have one. Sign in with your password and, if you use one, a code from your authenticator app.
- **Links open in your browser.** Public links and invite links still point at the server, since that's where other people open them.
- **The page can't reach anything native.** It gets no Tauri permissions; the Rust side only opens outside links and saves downloads to your Downloads folder.

The server lets the app in with CORS for its origins only (`tauri://localhost`, `http(s)://tauri.localhost`); sessions are bearer tokens, so there are no cookies for another origin to ride on.

## Building

You need what `build.sh` needs (rustup with `wasm32-unknown-unknown`, wasm-pack, Node.js), the [Tauri prerequisites](https://tauri.app/start/prerequisites/) for your system, and the Tauri CLI:

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
