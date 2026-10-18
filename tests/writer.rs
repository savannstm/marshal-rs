use marshal_rs::wire::Tag;
use marshal_rs::writer::{SliceSink, WriteError, Writer};

/// Runs `emit` on a fresh writer and checks exactly what it wrote.
fn check(expected: &[u8], emit: impl FnOnce(&mut Writer<SliceSink<'_>>) -> Result<(), WriteError>) {
    let mut buf = [0u8; 32];
    let mut writer = Writer::new(SliceSink::new(&mut buf));
    emit(&mut writer).unwrap();
    assert_eq!(writer.into_inner().written(), expected);
}

#[test]
fn slice_sink_reports_full() {
    let mut buf = [0u8; 2];
    let mut w = Writer::new(SliceSink::new(&mut buf));
    assert!(w.write_header().is_ok());
    assert_eq!(w.write_nil(), Err(WriteError::BufferFull));
}

#[test]
fn fixnum_roundtrip_bytes() {
    let mut buf = [0u8; 8];
    let mut w = Writer::new(SliceSink::new(&mut buf));
    w.write_header().unwrap();
    w.write_fixnum(1).unwrap();
    assert_eq!(w.into_inner().written(), &[4, 8, b'i', 1 + 5]);
}

#[cfg(feature = "std")]
#[test]
fn io_sink_writes_what_the_vec_sink_does() {
    use marshal_rs::writer::IoSink;
    let mut writer = Writer::new(IoSink(Vec::new()));
    writer.write_header().unwrap();
    writer.write_fixnum(300).unwrap();
    assert_eq!(writer.into_inner().0, b"\x04\x08i\x02\x2c\x01");
}

#[test]
fn scalar_primitives_write_their_bytes() {
    check(b"x", |w| w.write_byte(b'x'));
    check(&[1, 2], |w| w.write_bytes(&[1, 2]));
    check(b"[", |w| w.write_tag(Tag::Array));
    check(&[0], |w| w.write_int(0));
    check(&[6], |w| w.write_int(1));
    check(&[0xfa], |w| w.write_int(-1));
    check(&[2, 0x2c, 0x01], |w| w.write_int(300));
    check(&[8], |w| w.write_len(3));
    check(b"0", |w| w.write_nil());
    check(b"T", |w| w.write_bool(true));
    check(b"F", |w| w.write_bool(false));
}

#[test]
fn length_prefixed_records_write_their_bytes() {
    check(&[7, b'a', b'b'], |w| w.write_chunk(b"ab"));
    check(&[b'"', 7, b'h', b'i'], |w| w.write_string_bytes(b"hi"));
    check(&[b'f', 8, b'1', b'.', b'5'], |w| w.write_float(b"1.5"));
    check(&[b'/', 6, b'a', 1], |w| w.write_regexp(b"a", 1));
    check(&[b'c', 6, b'A'], |w| w.write_class_name(b"A"));
    check(&[b'm', 6, b'M'], |w| w.write_module_name(b"M", false));
    check(&[b'M', 6, b'M'], |w| w.write_module_name(b"M", true));
    check(&[b':', 6, b'a'], |w| w.write_symbol_new(b"a"));
}

#[test]
fn links_and_headers_write_their_bytes() {
    check(&[b';', 7], |w| w.write_symbol_link(2));
    check(&[b'@', 6], |w| w.write_object_link(1));
    check(&[b'[', 7], |w| w.write_array_header(2));
    check(&[b'{', 6], |w| w.write_hash_header(1, false));
    check(&[b'}', 6], |w| w.write_hash_header(1, true));
    check(b"I", |w| w.write_ivar_wrap_tag());
    check(b"e", |w| w.write_extended_tag());
    check(b"C", |w| w.write_uclass_tag());
}

#[test]
fn bignums_pad_odd_magnitudes_to_whole_words() {
    check(&[b'l', b'+', 7, 1, 0, 2, 0], |w| w.write_bignum(false, &[1, 0, 2]));
    check(&[b'l', b'-', 6, 0x34, 0x12], |w| w.write_bignum(true, &[0x34, 0x12]));
}
