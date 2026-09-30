//! `thencloud verify-web`'s Sigstore bundle check on hostile input: a
//! bundle comes with a download, so nothing in it may panic, and nothing
//! but a real signature may pass.
#![no_main]

use libfuzzer_sys::fuzz_target;
use thencloud_cli::sigstore::{GITHUB_ACTIONS, Identity, verify};

fuzz_target!(|data: &[u8]| {
    let who = Identity {
        name: "https://github.com/0x00ACAB/thencloud/.github/workflows/release.yml@refs/tags/v0.0.0",
        issuer: GITHUB_ACTIONS,
    };
    assert!(verify(data, b"not a signed file", &who).is_err());
});
