use marshal_rs::{dump, load};
#[cfg(not(feature = "sonic"))]
use serde_json::json;
#[cfg(feature = "sonic")]
use sonic_rs::json;

fn main() {
    // Bytes slice of Ruby Marshal data
    // Files with Marshal data can be read with std::fs::read()
    let null_bytes: [u8; 3] = [0x04, 0x08, 0x30]; // null

    // Serialize bytes to a Value
    // If "sonic" feature is enabled, returns sonic_rs::Value, otherwise serde_json::Value
    let json = load(&null_bytes, None, None).unwrap();
    assert_eq!(json, json!(null));

    // Here you may write the json object to file using std::fs::write()

    // Deserialize object back to bytes
    let marshal_bytes: Vec<u8> = dump(json, None);
    assert_eq!(&marshal_bytes, &null_bytes);

    // Here you may write bytes back to the Marshal file
}
