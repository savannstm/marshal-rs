//! Standard-alphabet, padded base64: how the serde envelope carries binary payloads.

use alloc::{string::String, vec::Vec};

const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

pub fn encode(bytes: &[u8]) -> String {
    let mut out = Vec::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let [a, b, c] = [chunk[0], *chunk.get(1).unwrap_or(&0), *chunk.get(2).unwrap_or(&0)];
        let word = u32::from(a) << 16 | u32::from(b) << 8 | u32::from(c);
        out.push(ALPHABET[(word >> 18) as usize & 63]);
        out.push(ALPHABET[(word >> 12) as usize & 63]);
        out.push(if chunk.len() > 1 {
            ALPHABET[(word >> 6) as usize & 63]
        } else {
            b'='
        });
        out.push(if chunk.len() > 2 {
            ALPHABET[word as usize & 63]
        } else {
            b'='
        });
    }
    // SAFETY: every byte pushed above is ASCII.
    unsafe { String::from_utf8_unchecked(out) }
}

/// `None` unless `text` is canonical padded base64.
pub fn decode(text: &str) -> Option<Vec<u8>> {
    let bytes = text.as_bytes();
    if bytes.len() % 4 != 0 {
        return None;
    }
    let sextet = |c: u8| -> Option<u32> {
        match c {
            b'A'..=b'Z' => Some(u32::from(c - b'A')),
            b'a'..=b'z' => Some(u32::from(c - b'a') + 26),
            b'0'..=b'9' => Some(u32::from(c - b'0') + 52),
            b'+' => Some(62),
            b'/' => Some(63),
            _ => None,
        }
    };
    let mut out = Vec::with_capacity(bytes.len() / 4 * 3);
    let last = bytes.len().checked_sub(4);
    for (index, quad) in bytes.chunks_exact(4).enumerate() {
        let pad = if Some(index * 4) == last {
            quad.iter().rev().take_while(|&&c| c == b'=').count()
        } else {
            0
        };
        if pad > 2 {
            return None;
        }
        let mut word = 0u32;
        for &c in &quad[..4 - pad] {
            word = word << 6 | sextet(c)?;
        }
        word <<= 6 * pad;
        out.extend_from_slice(&word.to_be_bytes()[1..4 - pad]);
    }
    Some(out)
}
