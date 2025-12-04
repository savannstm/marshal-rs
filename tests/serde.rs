use marshal_rs::Value;
use std::str::FromStr;

#[test]
fn test() {
    let value = Value::array([Value::int(1), Value::bool(true), Value::null()]);

    let serialized = value.to_string();
    Value::from_str(&serialized).unwrap();
}
