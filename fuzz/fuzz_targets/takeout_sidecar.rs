//! Google Takeout's JSON sidecars and file names come from an export the
//! user downloaded; reading them must never panic.
#![no_main]

use libfuzzer_sys::fuzz_target;
use thencloud_cli::takeout::{parse_sidecar, sidecar_names};

fuzz_target!(|data: &[u8]| {
    let d = parse_sidecar(data);
    for t in [d.taken, d.created].into_iter().flatten() {
        assert!((-2_208_988_800_000..7_258_118_400_000).contains(&t));
    }
    if let Ok(name) = std::str::from_utf8(data) {
        for s in sidecar_names(name) {
            assert!(s.ends_with(".json"));
        }
    }
});
