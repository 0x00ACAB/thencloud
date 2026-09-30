//! Sealed boxes (hybrid X25519 + ML-KEM-768) on hostile input: nothing may
//! panic, and a changed box must never open. Slow per run (each open is an
//! ML-KEM decapsulation), so it has its own target.
#![no_main]

use std::sync::LazyLock;

use libfuzzer_sys::fuzz_target;
use thencloud_crypto::{self as c, Key, KeyPair, PqKeyPair};

static PAIR: LazyLock<KeyPair> = LazyLock::new(|| {
    KeyPair::from_secret(Key::from_slice(&[9; 32]).unwrap())
        .with_pq(PqKeyPair::from_seed(&[5; c::PQ_SEED_LEN]).unwrap())
});
static X25519_ONLY: LazyLock<KeyPair> =
    LazyLock::new(|| KeyPair::from_secret(Key::from_slice(&[9; 32]).unwrap()));
static SEALED: LazyLock<Vec<u8>> =
    LazyLock::new(|| c::seal_to_public(&PAIR.sealing_key(), b"secret", b"aad").unwrap());
const ID: &str = "0b7c2d4e-8f9a-4b1c-9d2e-3f4a5b6c7d8e";

fuzz_target!(|data: &[u8]| {
    let _ = c::open_sealed(&PAIR, data, b"aad");
    let _ = c::open_sealed(&X25519_ONLY, data, b"aad");
    let _ = c::open_drop_key(&PAIR, data, ID, ID);
    let _ = c::open_report(&PAIR, data, ID, ID);
    let _ = c::PqKeyPair::from_seed(data);
    let _ = c::seal_to_public(data, b"secret", b"aad");

    let mut sealed = SEALED.clone();
    let before = sealed.clone();
    for pair in data.chunks_exact(3) {
        let at = u16::from_le_bytes([pair[0], pair[1]]) as usize;
        sealed[at % before.len()] ^= pair[2];
    }
    if sealed != before {
        assert!(c::open_sealed(&PAIR, &sealed, b"aad").is_err());
    }
});
