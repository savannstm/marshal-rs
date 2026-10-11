#![no_main]
use libfuzzer_sys::fuzz_target;
use marshal_rs::bignum::{decimal_to_le_bytes, le_bytes_to_decimal, MAX_CONVERTIBLE_BYTES};

fuzz_target!(|data: &[u8]| {
    // Bytes as a magnitude and a sign. The conversion is quadratic, so the library caps what it
    // accepts (`MAX_CONVERTIBLE_BYTES`); so does the target.
    if let Some((&sign, magnitude)) = data.split_first() {
        let magnitude = &magnitude[..magnitude.len().min(MAX_CONVERTIBLE_BYTES)];
        let text = le_bytes_to_decimal(sign & 1 == 1, magnitude);

        if let Some((_, back)) = decimal_to_le_bytes(&text) {
            let (mut trimmed, mut original) = (back.as_slice(), magnitude);
            while let [rest @ .., 0] = trimmed {
                trimmed = rest;
            }
            while let [rest @ .., 0] = original {
                original = rest;
            }
            assert!(trimmed == original || (trimmed.is_empty() && original.is_empty()));
        }
    }

    // And text as a decimal.
    if let Ok(text) = std::str::from_utf8(data) {
        let _ = decimal_to_le_bytes(text);
    }
});
