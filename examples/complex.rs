use marshal_rs::{dump::Dumper, load::Loader};
use serde_json::json;

fn main() {
    // Bytes slice of Ruby Marshal data
    // Files with Marshal data can be read with std::fs::read()
    let null: [u8; 3] = [0x04, 0x08, 0x30]; // null
    let true_: [u8; 3] = [0x04, 0x08, 0x54]; // true
    let false_: [u8; 3] = [0x04, 0x08, 0x46]; // false

    // Initialize the loader, might be more efficient to loading multiple files
    let mut loader: Loader = Loader::new();

    // Load the values of multiple objects
    let null_value: serde_json::Value = loader.load(&null, None);
    let true_value: serde_json::Value = loader.load(&true_, None);
    let false_value: serde_json::Value = loader.load(&false_, None);

    assert_eq!(null_value, json!(null));
    assert_eq!(true_value, json!(true));
    assert_eq!(false_value, json!(false));

    // Here you may write the json object to file using std::fs::write()

    // Initialize the dumper, might be more efficient to dump multiple files
    let mut dumper: Dumper = Dumper::new();

    // Serialize objects back to Marshal bytes
    let null_marshal: Vec<u8> = dumper.dump(null_value, None);
    let true_marshal: Vec<u8> = dumper.dump(true_value, None);
    let false_marshal: Vec<u8> = dumper.dump(false_value, None);

    assert_eq!(&null_marshal, &null);
    assert_eq!(&true_marshal, &true_);
    assert_eq!(&false_marshal, &false_);

    // Here you may write bytes back to the Marshal file
}
