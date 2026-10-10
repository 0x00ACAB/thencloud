//! Passkey registration: client data JSON, the CBOR attestation object,
//! authenticator data and COSE keys, all from the browser. Nothing may
//! panic.
#![no_main]

use libfuzzer_sys::fuzz_target;
use thencloud_server::webauthn;

fuzz_target!(|data: &[u8]| {
    if data.len() < 2 {
        return;
    }
    let split = (u16::from_le_bytes([data[0], data[1]]) as usize).min(data.len() - 2);
    let (client_data, attestation) = data[2..].split_at(split);
    let _ = webauthn::register(client_data, attestation, b"challenge", &[]);
    // Also with client data that passes, so the CBOR gets parsed.
    let good = br#"{"type":"webauthn.create","challenge":"Y2hhbGxlbmdl","origin":"https://cloud.example.com"}"#;
    let _ = webauthn::register(good, &data[2..], b"challenge", &[]);
});
