//! Passkey sign-in: a stored COSE key (from registration) and the
//! authenticator data and signature from the browser. Nothing may panic.
#![no_main]

use libfuzzer_sys::fuzz_target;
use thencloud_server::webauthn::{self, Stored};

fuzz_target!(|data: &[u8]| {
    if data.len() < 4 {
        return;
    }
    let a = (data[0] as usize).min(data.len() - 4);
    let rest = &data[4..];
    let b = (u16::from_le_bytes([data[1], data[2]]) as usize).min(rest.len());
    let (public_key, rest) = rest.split_at(a.min(rest.len()));
    let (auth_data, signature) = rest.split_at(b.min(rest.len()));
    let cred = Stored {
        public_key,
        rp_id: "cloud.example.com",
        sign_count: data[3] as u32,
    };
    let good = br#"{"type":"webauthn.get","challenge":"Y2hhbGxlbmdl","origin":"https://cloud.example.com"}"#;
    let _ = webauthn::assert(
        &cred,
        good,
        auth_data,
        signature,
        b"challenge",
        data[3] & 1 == 1,
    );
    let _ = webauthn::assert(&cred, auth_data, signature, public_key, b"challenge", false);
});
