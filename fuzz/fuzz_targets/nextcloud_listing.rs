//! The Nextcloud importer's PROPFIND parser on hostile input: nothing may
//! panic, and every item it returns must be a plain name that stays in the
//! folder being listed.
#![no_main]

use libfuzzer_sys::fuzz_target;
use thencloud_cli::nextcloud::parse_listing;

fuzz_target!(|data: &[u8]| {
    if let Ok(items) = parse_listing(data, "/remote.php/dav/files/alice/Docs/") {
        for item in items {
            assert!(!item.name.is_empty());
            assert!(!item.name.contains('/'));
            assert!(item.name != "." && item.name != "..");
            assert!(!item.name.chars().any(char::is_control));
            assert!(!item.folder || item.size == 0);
        }
    }
});
