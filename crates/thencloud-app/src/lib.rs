//! The thencloud app: the web client, built with `npm run build:app` and
//! bundled into the binary, in a Tauri webview. Nothing is loaded from the
//! server but data, so a compromised server can't hand the app new code.
//!
//! The page gets no Tauri permissions but one, on Android only: saving a file
//! it downloads (capabilities/android.json; Android's WebView can't save a
//! blob: URL itself). Links that leave the app open in the system browser.

use tauri::webview::WebviewWindowBuilder;
use tauri::{Url, WebviewUrl};
use tauri_plugin_opener::OpenerExt;

/// The app's own pages: `tauri://localhost` on Linux, `http://tauri.localhost`
/// on Windows and Android, and the Vite dev server in a debug build.
fn is_app(url: &Url, dev: Option<&Url>) -> bool {
    matches!(url.scheme(), "tauri" | "blob" | "about")
        || url.host_str() == Some("tauri.localhost")
        || dev.is_some_and(|d| d.origin() == url.origin())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_thencloud::init())
        .setup(|app| {
            let handle = app.handle().clone();
            let dev = cfg!(debug_assertions)
                .then(|| app.config().build.dev_url.clone())
                .flatten();
            let window = WebviewWindowBuilder::new(app, "main", WebviewUrl::default())
                .on_navigation(move |url| {
                    if is_app(url, dev.as_ref()) {
                        return true;
                    }
                    if matches!(url.scheme(), "http" | "https" | "mailto") {
                        let _ = handle.opener().open_url(url.as_str(), None::<&str>);
                    }
                    false
                })
                // Saved to Downloads under the name the page gave. Not on
                // Android, where the page saves through the plugin instead.
                .on_download(|_, _| true);
            #[cfg(desktop)]
            let window = window
                .title("thencloud")
                .inner_size(1200.0, 800.0)
                .min_inner_size(360.0, 480.0);
            window.build()?;
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running thencloud");
}
