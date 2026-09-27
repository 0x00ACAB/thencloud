//! Time-based one-time passwords (RFC 6238): HMAC-SHA1, 30-second steps,
//! six digits, which is what authenticator apps expect.

use ring::hmac;

pub const STEP: i64 = 30;
pub const SECRET_LEN: usize = 20;

pub fn code(secret: &[u8], step: i64) -> u32 {
    let key = hmac::Key::new(hmac::HMAC_SHA1_FOR_LEGACY_USE_ONLY, secret);
    let mac = hmac::sign(&key, &(step as u64).to_be_bytes());
    let h = mac.as_ref();
    let o = (h[h.len() - 1] & 0x0f) as usize;
    let n = u32::from_be_bytes([h[o] & 0x7f, h[o + 1], h[o + 2], h[o + 3]]);
    n % 1_000_000
}

/// The time step a code is valid for, if it matches the current step or
/// one either side (clock drift) and is newer than `last_step`.
pub fn check(secret: &[u8], input: &str, now: i64, last_step: Option<i64>) -> Option<i64> {
    let digits: String = input.chars().filter(|c| !c.is_whitespace()).collect();
    if digits.len() != 6 || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let want: u32 = digits.parse().ok()?;
    let t = now / STEP;
    (t - 1..=t + 1)
        .filter(|s| last_step.is_none_or(|l| *s > l))
        .find(|s| code(secret, *s) == want)
}

/// RFC 4648 base32 without padding.
pub fn base32(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 32] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";
    let (mut out, mut acc, mut bits) = (String::new(), 0u32, 0u32);
    for &b in bytes {
        acc = (acc << 8) | b as u32;
        bits += 8;
        while bits >= 5 {
            bits -= 5;
            out.push(ALPHABET[((acc >> bits) & 31) as usize] as char);
        }
    }
    if bits > 0 {
        out.push(ALPHABET[((acc << (5 - bits)) & 31) as usize] as char);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rfc6238_vectors() {
        let secret = b"12345678901234567890";
        // Appendix B, SHA-1, truncated to six digits.
        for (time, want) in [
            (59, 287082),
            (1111111109, 81804),
            (1234567890, 5924),
            (2000000000, 279037),
        ] {
            assert_eq!(code(secret, time / STEP), want, "t={time}");
        }
        assert_eq!(check(secret, "287 082", 59, None), Some(1));
        assert_eq!(check(secret, "287082", 59 + 30, None), Some(1));
        assert_eq!(check(secret, "287082", 59 + 90, None), None);
        // Used once, not again.
        assert_eq!(check(secret, "287082", 59, Some(1)), None);
        assert_eq!(check(secret, "28708", 59, None), None);
    }

    #[test]
    fn base32_matches_rfc4648() {
        assert_eq!(base32(b"foobar"), "MZXW6YTBOI");
        assert_eq!(base32(b"f"), "MY");
    }
}
