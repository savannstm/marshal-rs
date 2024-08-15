use marshal_rs::{dump::dump, load::load};
use serde_json::json;

fn main() {
    // Bytes slice of Ruby Marshal data
    // Files with Marshal data can be read with std::fs::read()
    let bytes: [u8; 3] = [0x04, 0x08, 0x30]; // null

    // Serialize bytes to a Value
    // If "sonic" feature is enabled, returns sonic_rs::Value, otherwise serde_json::Value
    let json: serde_json::Value = load(&bytes);
    assert_eq!(json, json!(null));

    // Here you may write the json object to file using std::fs::write()

    // Deserialize object back to bytes
    let marshal: Vec<u8> = dump(json);
    assert_eq!(&marshal, &bytes);

    // Here you may write bytes back to the Marshal file
}
