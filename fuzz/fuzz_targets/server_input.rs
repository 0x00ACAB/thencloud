//! The server's small parsers of client input: TOTP codes, usernames, ids,
//! and the size checks on encrypted fields. Nothing may panic.
#![no_main]

use libfuzzer_sys::fuzz_target;
use thencloud_crypto::api::B64;
use thencloud_server::{totp, util};

fuzz_target!(|data: &[u8]| {
    let text = String::from_utf8_lossy(data);
    let _ = totp::check(
        b"12345678901234567890",
        &text,
        1_790_000_000,
        Some(59_666_665),
    );
    let _ = util::normalize_username(&text);
    let _ = util::check_id(&text, "id");
    let _ = util::check_sealed(data, "key");
    let _ = util::check_metadata(data);
    let _ = util::check_name_tag(&Some(B64(data.to_vec())));
    let half = data.len() / 2;
    let _ = util::check_pq_key(
        &Some(B64(data[..half].to_vec())),
        &Some(B64(data[half..].to_vec())),
    );
    let _ = util::hmac_verify_prefix(b"secret", b"msg", data);
});
