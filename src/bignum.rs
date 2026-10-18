//! Bignum <-> decimal-string conversion over plain byte arrays.

use alloc::{string::String, vec::Vec};

/// Largest magnitude, in bytes, converted to decimal. The conversion is quadratic (64 KiB takes
/// about half a minute), so the limit keeps hostile input from stalling callers. 4096 bytes takes ~100 ms.
pub const MAX_CONVERTIBLE_BYTES: usize = 4096;

/// Most digits [`decimal_to_le_bytes`] accepts, not counting the sign. A [`MAX_CONVERTIBLE_BYTES`] magnitude has
/// at most 9,865 decimal digits; this limit rounds that up.
pub const MAX_CONVERTIBLE_DIGITS: usize = 10_000;

/// Converts a little-endian magnitude to a decimal string, with `-` if `negative` and nonzero.
/// Quadratic: keep untrusted input within [`MAX_CONVERTIBLE_BYTES`].
#[allow(clippy::cast_possible_truncation)]
#[must_use]
pub fn le_bytes_to_decimal(negative: bool, magnitude_le: &[u8]) -> String {
    // Trim trailing (most-significant) zero bytes.
    let mut end = magnitude_le.len();
    while end > 0 && magnitude_le[end - 1] == 0 {
        end -= 1;
    }
    if end == 0 {
        return String::from("0");
    }

    let mut digits = magnitude_le[..end].to_vec();
    let mut decimal = Vec::<u8>::new();

    while !digits.iter().all(|&b| b == 0) {
        let mut remainder: u32 = 0;
        for byte in digits.iter_mut().rev() {
            let acc = (remainder << 8) | u32::from(*byte);
            *byte = (acc / 10) as u8;
            remainder = acc % 10;
        }
        decimal.push(b'0' + remainder as u8);
    }

    let mut out = String::with_capacity(decimal.len() + 1);
    if negative {
        out.push('-');
    }
    for &d in decimal.iter().rev() {
        out.push(d as char);
    }
    out
}

/// `magnitude_le` without its high zero bytes. Ruby's integers never carry them, so a stream padded with them
/// loads as the same number and dumps without the padding. Zero is the empty magnitude.
#[must_use]
pub(crate) fn normalize_magnitude(magnitude_le: &[u8]) -> &[u8] {
    let end = magnitude_le
        .iter()
        .rposition(|&byte| byte != 0)
        .map_or(0, |last| last + 1);
    &magnitude_le[..end]
}

/// Parses a decimal string (optionally `-`-prefixed) into a little-endian
/// magnitude and its sign. Returns `None` if `s` is not a valid decimal
/// integer literal.
#[must_use]
pub fn decimal_to_le_bytes(s: &str) -> Option<(bool, Vec<u8>)> {
    let (negative, digits) = s.strip_prefix('-').map_or((false, s), |rest| (true, rest));
    if digits.is_empty() || digits.len() > MAX_CONVERTIBLE_DIGITS || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }

    let mut magnitude: Vec<u8> = alloc::vec![0];
    for byte in digits.bytes() {
        let digit = u32::from(byte - b'0');
        let mut carry = digit;
        for limb in &mut magnitude {
            let acc = u32::from(*limb) * 10 + carry;
            *limb = (acc & 0xff) as u8;
            carry = acc >> 8;
        }
        while carry > 0 {
            magnitude.push((carry & 0xff) as u8);
            carry >>= 8;
        }
    }

    while magnitude.len() > 1 && magnitude.last() == Some(&0) {
        magnitude.pop();
    }
    let negative = negative && magnitude != [0];
    Some((negative, magnitude))
}
