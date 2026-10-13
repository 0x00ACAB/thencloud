// The commands the Kotlin side implements (android/src/main/java). Each gets
// an `allow-*` permission; the app grants them on Android only.
const COMMANDS: &[&str] = &[
    "save_begin",
    "save_write",
    "save_end",
    "media_update",
    "media_artwork",
    "media_stop",
    "network_status",
    "share_pending",
    "share_read",
    "share_done",
    "share_out_begin",
    "share_out_write",
    "share_out_end",
];

fn main() {
    tauri_plugin::Builder::new(COMMANDS)
        .android_path("android")
        .build();
}
