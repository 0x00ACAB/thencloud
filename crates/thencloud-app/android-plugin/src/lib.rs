//! Native pieces the Android app needs that a WebView doesn't do by itself:
//! saving downloads (Android's WebView drops `blob:` downloads) and telling
//! the page where the system bars and the keyboard are. All of it is Kotlin,
//! in `android/`; on other platforms this plugin does nothing.

use tauri::Runtime;
use tauri::plugin::{Builder, TauriPlugin};

pub fn init<R: Runtime>() -> TauriPlugin<R> {
    Builder::new("thencloud")
        .setup(|_app, _api| {
            #[cfg(target_os = "android")]
            _api.register_android_plugin("org.thencloud.plugin", "ThencloudPlugin")?;
            Ok(())
        })
        .build()
}
