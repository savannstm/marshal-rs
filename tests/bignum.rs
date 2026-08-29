use marshal_rs::bignum::*;

#[test]
fn zero() {
    assert_eq!(le_bytes_to_decimal(false, &[]), "0");
    assert_eq!(le_bytes_to_decimal(true, &[0, 0]), "0");
}

#[test]
fn roundtrip() {
    for s in [
        "0",
        "1",
        "255",
        "256",
        "36893488147419103232",
        "-1",
        "-36893488147419103232",
    ] {
        let (neg, bytes) = decimal_to_le_bytes(s).unwrap();
        assert_eq!(le_bytes_to_decimal(neg, &bytes), s);
    }
}

#[test]
fn rejects_garbage() {
    assert!(decimal_to_le_bytes("").is_none());
    assert!(decimal_to_le_bytes("12a").is_none());
    assert!(decimal_to_le_bytes("-").is_none());
}
