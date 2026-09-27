//! The crypto crate's symmetric decoders on hostile input: nothing may
//! panic, and a changed ciphertext must never open to anything but an
//! error. Sealed boxes are in `sealed_box.rs`, which is slower.
#![no_main]

use std::sync::LazyLock;

use libfuzzer_sys::fuzz_target;
use thencloud_crypto::{self as c, Key, Metadata};

static KEY: LazyLock<Key> = LazyLock::new(|| Key::from_slice(&[7; 32]).unwrap());
const ID: &str = "0b7c2d4e-8f9a-4b1c-9d2e-3f4a5b6c7d8e";
const PLAIN: &[u8] = b"the quick brown fox";
static CHUNK: LazyLock<Vec<u8>> = LazyLock::new(|| c::encrypt_chunk(&KEY, ID, 3, false, PLAIN));
static WRAPPED: LazyLock<Vec<u8>> = LazyLock::new(|| c::wrap_node_key(&KEY, &KEY, ID));

/// Flip bytes of `sealed` at the positions and masks `data` gives. True
/// if that changed it (two flips of the same bits cancel out).
fn tamper(sealed: &mut [u8], data: &[u8]) -> bool {
    let before = sealed.to_vec();
    for pair in data.chunks_exact(3) {
        let at = u16::from_le_bytes([pair[0], pair[1]]) as usize;
        if !sealed.is_empty() {
            sealed[at % sealed.len()] ^= pair[2];
        }
    }
    *sealed != *before
}

fuzz_target!(|data: &[u8]| {
    let text = String::from_utf8_lossy(data);

    // Raw decoders.
    let _ = c::b64_decode(&text);
    let _ = Key::from_b64(&text);
    let _ = Key::from_slice(data);
    let _ = c::decode_recovery_key(&text);
    let _ = c::open(&KEY, data, b"aad");
    let _ = c::unwrap_node_key(&KEY, data, ID);
    let _ = c::unwrap_content_key(&KEY, data, ID, ID);
    let _ = c::decrypt_chunk(&KEY, ID, 0, true, data);
    let _ = c::decrypt_metadata(&KEY, ID, data);
    let _ = c::decrypt_private_data(&KEY, ID, "music", data);
    let _ = c::unwrap_master_key(&KEY, data);
    let _ = c::unwrap_private_key(&KEY, data);
    let _ = c::unwrap_pq_private_key(&KEY, data);
    let _ = c::identity(&[1; 32], Some(data));
    let _ = c::fingerprint(data);

    // What decrypted metadata goes through after the tag checks out.
    if let Ok(m) = serde_json::from_slice::<Metadata>(data) {
        let _ = m.validate();
    }

    // Tampering: a changed chunk, key or sealed box must not open.
    let mut chunk = CHUNK.clone();
    if tamper(&mut chunk, data) {
        assert!(c::decrypt_chunk(&KEY, ID, 3, false, &chunk).is_err());
    }
    let mut wrapped = WRAPPED.clone();
    if tamper(&mut wrapped, data) {
        assert!(c::unwrap_node_key(&KEY, &wrapped, ID).is_err());
    }
});
