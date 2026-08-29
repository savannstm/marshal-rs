use marshal_rs::writer::{SliceSink, WriteError, Writer};

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
