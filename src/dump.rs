use crate::{Constants, DEFAULT_SYMBOL, ENCODING_SHORT_SYMBOL, EXTENDS_SYMBOL, MARSHAL_VERSION};
use cfg_if::cfg_if;
use std::mem;
cfg_if! {
    if #[cfg(feature = "sonic")] {
        use sonic_rs::{
            from_value, Array, JsonContainerTrait, JsonType, JsonValueMutTrait, JsonValueTrait, Object,
            Value,
        };
    } else {
        use serde_json::{from_value, Value};
    }
}

pub struct Dumper {
    buffer: Vec<u8>,
    symbols: Vec<Value>,
}

impl Dumper {
    pub fn new() -> Self {
        Self {
            buffer: Vec::with_capacity(128),
            symbols: Vec::new(),
        }
    }

    /// Serializes JSON object to a Marshal byte stream.
    /// /// # Example
    /// ```rust
    /// use marshal_rs::dump::Dumper;
    /// use serde_json::{Value, json};
    ///
    /// // Initialize dumper
    /// let mut dumper = Dumper::new();
    ///
    /// // Value of null
    /// let json: serde_json::Value = json!(null); // null
    ///
    /// // Serialize Value to bytes
    /// let bytes: Vec<u8> = dumper.dump(json);
    /// assert_eq!(&bytes, &[0x04, 0x08, 0x30]);
    /// ```
    pub fn dump(&mut self, value: Value) -> Vec<u8> {
        write_buffer(&MARSHAL_VERSION.to_be_bytes(), &mut self.buffer);
        write_structure(value, &mut self.symbols, &mut self.buffer);

        self.symbols.clear();

        mem::take(&mut self.buffer)
    }
}

impl Default for Dumper {
    fn default() -> Self {
        Self::new()
    }
}

fn write_byte(byte: Constants, buffer: &mut Vec<u8>) {
    buffer.push(byte as u8);
}

fn write_buffer(bytes: &[u8], buffer: &mut Vec<u8>) {
    buffer.extend(bytes);
}

fn write_bytes(bytes: &[u8], buffer: &mut Vec<u8>) {
    write_fixnum(bytes.len() as i32, buffer);
    write_buffer(bytes, buffer);
}

fn write_fixnum(number: i32, buffer: &mut Vec<u8>) {
    let mut buf: Vec<u8> = vec![0; 5];
    let end: i8 = write_marshal_fixnum(number, &mut buf);

    write_buffer(&buf[0..end as usize], buffer);
}

fn write_marshal_fixnum(mut fixnum: i32, buffer: &mut [u8]) -> i8 {
    match fixnum {
        0 => {
            buffer[0] = 0;
            1
        }
        1..123 => {
            buffer[0] = (fixnum + 5) as u8;
            1
        }
        -123..0 => {
            buffer[0] = ((fixnum - 5) & 0xff) as u8;
            1
        }
        _ => {
            let mut i: i8 = 1;

            while i < 5 {
                buffer[i as usize] = (fixnum & 0xff) as u8;
                fixnum >>= 8;

                match fixnum {
                    0 => {
                        buffer[0] = i as u8;
                        break;
                    }
                    -1 => {
                        buffer[0] = -i as u8;
                        break;
                    }
                    _ => {}
                }

                i += 1;
            }

            i + 1
        }
    }
}

fn write_string(string: &str, buffer: &mut Vec<u8>) {
    write_bytes(string.as_bytes(), buffer)
}

fn write_float(float: f64, buffer: &mut Vec<u8>) {
    let string: String = float.to_string();

    write_string(
        if float.is_infinite() {
            if float.is_sign_positive() {
                "inf"
            } else {
                "-inf"
            }
        } else if float.is_sign_negative() && float == 0f64 {
            "-0"
        } else {
            string.as_str()
        },
        buffer,
    );
}

fn write_symbol(mut symbol: Value, symbols: &mut Vec<Value>, buffer: &mut Vec<u8>) {
    if let Some(stripped) = symbol.as_str().unwrap().strip_prefix("__symbol__") {
        symbol = stripped.into();
    }

    if let Some(pos) = symbols.iter().position(|sym: &Value| sym == &symbol) {
        write_byte(Constants::Symlink, buffer);
        write_fixnum(pos as i32, buffer);
    } else {
        write_byte(Constants::Symbol, buffer);
        write_bytes(symbol.as_str().unwrap().as_bytes(), buffer);
        symbols.push(symbol);
    }
}

fn write_extended(extended: Vec<Value>, symbols: &mut Vec<Value>, buffer: &mut Vec<u8>) {
    for symbol in extended {
        write_byte(Constants::Extended, buffer);
        write_symbol(symbol, symbols, buffer);
    }
}

fn write_class(
    data_type: Constants,
    object: &mut Value,
    symbols: &mut Vec<Value>,
    buffer: &mut Vec<u8>,
) {
    if !object[EXTENDS_SYMBOL].is_null() {
        cfg_if! {
            if #[cfg(feature = "sonic")] {
                write_extended(
                    from_value(&object[EXTENDS_SYMBOL]).unwrap(),
                    symbols,
                    buffer,
                );
            } else {
                write_extended(
                    from_value(object[EXTENDS_SYMBOL].take()).unwrap(),
                    symbols,
                    buffer,
                );
            }
        }
    }

    write_byte(data_type, buffer);
    write_symbol(object["__class"].take(), symbols, buffer);
}

fn write_user_class(buffer: &mut Vec<u8>, symbols: &mut Vec<Value>, object: &mut Value) {
    if !object[EXTENDS_SYMBOL].is_null() {
        cfg_if! {
            if #[cfg(feature = "sonic")] {
                write_extended(
                    from_value(&object[EXTENDS_SYMBOL]).unwrap(),
                    symbols,
                    buffer,
                );
            } else {
                write_extended(
                    from_value(object[EXTENDS_SYMBOL].take()).unwrap(),
                    symbols,
                    buffer,
                );
            }
        }
    }

    if !object["__wrapped"].is_null() {
        write_byte(Constants::UserClass, buffer);
        write_symbol(object["__class"].take(), symbols, buffer)
    }
}

fn write_instance_var(mut object: Value, symbols: &mut Vec<Value>, buffer: &mut Vec<u8>) {
    let object = object.as_object_mut().unwrap();

    for key in [
        "__class",
        "__type",
        "__data",
        "__wrapped",
        "__userDefined",
        "__userMarshal",
    ] {
        cfg_if! {
            if #[cfg(feature = "sonic")] {
                object.remove(&key);
            } else {
                object.remove(key);
            }
        }
    }

    let size: usize = object.len();

    if size > 0 {
        write_fixnum(size as i32, buffer);

        for (key, value) in object.iter_mut() {
            cfg_if! {
                if #[cfg(feature = "sonic")] {
                    write_symbol(key.into(), symbols, buffer);
                } else {
                    write_symbol(Value::from(key.as_str()), symbols, buffer);
                }
            }

            write_structure(value.take(), symbols, buffer);
        }
    } else {
        write_fixnum(0, buffer);
    }
}

fn write_bignum(mut bignum: i64, buffer: &mut Vec<u8>) {
    write_byte(Constants::Bignum, buffer);
    write_byte(
        if bignum < 0 {
            Constants::Negative
        } else {
            Constants::Positive
        },
        buffer,
    );

    let mut buf: Vec<u8> = Vec::new();
    bignum = bignum.abs();

    loop {
        buf.push((bignum & 0xff) as u8);
        bignum /= 256;

        if bignum == 0 {
            break;
        }
    }

    if buf.len() & 1 != 0 {
        buf.push(0);
    }

    write_fixnum(buf.len() as i32 >> 1, buffer);
    write_buffer(&buf, buffer);
}

fn write_structure(mut value: Value, symbols: &mut Vec<Value>, buffer: &mut Vec<u8>) {
    cfg_if! {
        if #[cfg(feature = "sonic")] {
            match value.get_type() {
                JsonType::Null => write_byte(Constants::Nil, buffer),
                JsonType::Boolean => {
                    write_byte(
                        if value.is_true() {
                            Constants::True
                        } else {
                            Constants::False
                        },
                        buffer,
                    );
                }
                JsonType::Number => {
                    if let Some(integer) = value.as_i64() {
                        if (-0x40_000_000..0x40_000_000).contains(&integer) {
                            write_byte(Constants::Fixnum, buffer);
                            write_fixnum(integer as i32, buffer);
                        } else {
                            write_bignum(integer, buffer);
                        }
                    } else if let Some(float) = value.as_f64() {
                        write_byte(Constants::Float, buffer);
                        write_float(float, buffer);
                    }
                }
                JsonType::Object => {
                    if let Some(object_type) = value["__type"].as_str() {
                        match object_type {
                            "bytes" => {
                                let buf: Vec<u8> = from_value(&value["data"]).unwrap();

                                write_byte(Constants::String, buffer);
                                write_bytes(&buf, buffer);
                            }
                            "object" => {
                                if value.get("__data").is_some() {
                                    write_class(Constants::Data, &mut value, symbols, buffer);
                                    write_structure(value["__data"].take(), symbols, buffer);
                                } else if value.get("__wrapped").is_some() {
                                    write_user_class(buffer, symbols, &mut value);
                                    write_structure(value["__wrapped"].take(), symbols, buffer);
                                } else if value.get("__userDefined").is_some() {
                                    let object: &Object = value.as_object().unwrap();
                                    let mut object_len: usize = object.len();

                                    for (key, _) in object {
                                        if key.starts_with("__") {
                                            object_len -= 1;
                                        }
                                    }

                                    let has_instance_var: bool = object_len > 0;

                                    if has_instance_var {
                                        write_byte(Constants::InstanceVar, buffer);
                                    }

                                    write_class(Constants::UserDefined, &mut value, symbols, buffer);
                                    write_bytes(
                                        &from_value::<Vec<u8>>(&value["__userDefined"]).unwrap(),
                                        buffer,
                                    );

                                    if has_instance_var {
                                        write_instance_var(value, symbols, buffer);
                                    }
                                } else if value.get("__userMarshal").is_some() {
                                    write_class(Constants::UserMarshal, &mut value, symbols, buffer);
                                    write_structure(value["__userMarshal"].take(), symbols, buffer);
                                } else {
                                    write_class(Constants::Object, &mut value, symbols, buffer);
                                    write_instance_var(value, symbols, buffer);
                                }
                            }
                            "struct" => {
                                write_class(Constants::Struct, &mut value, symbols, buffer);
                                write_instance_var(value["__members"].take(), symbols, buffer);
                            }
                            "class" => {
                                write_byte(Constants::Class, buffer);
                                write_string(value["__name"].take().as_str().unwrap(), buffer);
                            }
                            "module" => write_byte(
                                if value.get("__old").is_true() {
                                    Constants::ModuleOld
                                } else {
                                    Constants::Module
                                },
                                buffer,
                            ),
                            "regexp" => {
                                write_byte(Constants::Regexp, buffer);
                                write_string(value["expression"].as_str().unwrap(), buffer);

                                let flags = value["flags"].as_str().unwrap();

                                let options: Constants =  if flags == "im" {
                                    Constants::RegexpBoth
                                } else if flags.contains('i') {
                                    Constants::RegexpIgnore
                                } else if flags.contains('m') {
                                    Constants::RegexpMultiline
                                } else {
                                    Constants::RegexpNone
                                };

                                write_byte(options, buffer);
                            }
                            _ => {
                                let object: &mut Object = value.as_object_mut().unwrap();

                                write_byte(
                                    if object.get(&DEFAULT_SYMBOL).is_some() {
                                        Constants::HashDefault
                                    } else {
                                        Constants::Hash
                                    },
                                    buffer,
                                );

                                let entries: sonic_rs::value::object::IterMut = object.iter_mut();
                                write_fixnum(entries.len() as i32, buffer);

                                for (key, value) in entries {
                                    if let Some(stripped) = key.strip_prefix("__integer__") {
                                        write_structure(
                                            stripped.parse::<u16>().unwrap().into(),
                                            symbols,
                                            buffer,
                                        );
                                    } else if let Some(stripped) = key.strip_prefix("__object__") {
                                        write_structure(stripped.into(), symbols, buffer)
                                    } else {
                                        write_structure(key.into(), symbols, buffer);
                                    }

                                    write_structure(value.take(), symbols, buffer);
                                }
                            }
                        }
                    }
                }
                JsonType::Array => {
                    let array: &mut Array = value.as_array_mut().unwrap();
                    write_byte(Constants::Array, buffer);
                    write_fixnum(array.len() as i32, buffer);

                    for element in array {
                        write_structure(element.take(), symbols, buffer);
                    }
                }
                JsonType::String => {
                    let string: &str = value.as_str().unwrap();

                    if string.starts_with("__symbol__") {
                        write_symbol(string.into(), symbols, buffer);
                    } else {
                        write_byte(Constants::InstanceVar, buffer);
                        write_byte(Constants::String, buffer);
                        write_string(string, buffer);
                        write_fixnum(1, buffer);
                        write_symbol(ENCODING_SHORT_SYMBOL.into(), symbols, buffer);
                        write_byte(Constants::True, buffer);
                    }
                }
            }
        } else {
            match value {
                Value::Null => write_byte(Constants::Nil, buffer),
                Value::Bool(bool) => {
                    write_byte(
                        if bool {
                            Constants::True
                        } else {
                            Constants::False
                        },
                        buffer,
                    );
                }
                Value::Number(number) => {
                    if let Some(integer) = number.as_i64() {
                        if (-0x40_000_000..0x40_000_000).contains(&integer) {
                            write_byte(Constants::Fixnum, buffer);
                            write_fixnum(integer as i32, buffer);
                        } else {
                            write_bignum(integer, buffer);
                        }
                    } else if let Some(float) = number.as_f64() {
                        write_byte(Constants::Float, buffer);
                        write_float(float, buffer);
                    }
                }
                Value::Object(_) => {
                    if let Some(object_type) = value.get("__type") {
                        match object_type.as_str().unwrap() {
                            "bytes" => {
                                let buf: Vec<u8> = from_value(value["data"].take()).unwrap();

                                write_byte(Constants::String, buffer);
                                write_bytes(&buf, buffer);
                            }
                            "object" => {
                                if value.get("__data").is_some() {
                                    write_class(Constants::Data, &mut value, symbols, buffer);
                                    write_structure(value["__data"].take(), symbols, buffer);
                                } else if value.get("__wrapped").is_some() {
                                    write_user_class(buffer, symbols, &mut value);
                                    write_structure(value["__wrapped"].take(), symbols, buffer);
                                } else if value.get("__userDefined").is_some() {
                                    let object = value.as_object_mut().unwrap();
                                    let mut object_len: usize = object.len();

                                    for (key, _) in object {
                                        if key.starts_with("__") {
                                            object_len -= 1;
                                        }
                                    }

                                    let has_instance_var: bool = object_len > 0;

                                    if has_instance_var {
                                        write_byte(Constants::InstanceVar, buffer);
                                    }

                                    write_class(Constants::UserDefined, &mut value, symbols, buffer);
                                    write_bytes(
                                        &from_value::<Vec<u8>>(value["__userDefined"].take()).unwrap(),
                                        buffer,
                                    );

                                    if has_instance_var {
                                        write_instance_var(value, symbols, buffer);
                                    }
                                } else if value.get("__userMarshal").is_some() {
                                    write_class(Constants::UserMarshal, &mut value, symbols, buffer);
                                    write_structure(value["__userMarshal"].take(), symbols, buffer);
                                } else {
                                    write_class(Constants::Object, &mut value, symbols, buffer);
                                    write_instance_var(value, symbols, buffer);
                                }
                            }
                            "struct" => {
                                write_class(Constants::Struct, &mut value, symbols, buffer);
                                write_instance_var(value["__members"].take(), symbols, buffer);
                            }
                            "class" => {
                                write_byte(Constants::Class, buffer);
                                write_string(value["__name"].take().as_str().unwrap(), buffer);
                            }
                            "module" => write_byte(
                                if value.get("__old").is_some_and(|old| old.as_bool().unwrap()) {
                                    Constants::ModuleOld
                                } else {
                                    Constants::Module
                                },
                                buffer,
                            ),
                            "regexp" => {
                                write_byte(Constants::Regexp, buffer);
                                write_string(value["expression"].as_str().unwrap(), buffer);

                                let flags = value["flags"].as_str().unwrap();

                                let options: Constants =  if flags == "im" {
                                    Constants::RegexpBoth
                                } else if flags.contains('i') {
                                    Constants::RegexpIgnore
                                } else if flags.contains('m') {
                                    Constants::RegexpMultiline
                                } else {
                                    Constants::RegexpNone
                                };

                                write_byte(options, buffer);
                            }
                            _ => {
                                let object = value.as_object_mut().unwrap();

                                write_byte(
                                    if object.get(DEFAULT_SYMBOL).is_some() {
                                        Constants::HashDefault
                                    } else {
                                        Constants::Hash
                                    },
                                    buffer,
                                );

                                let entries = object.iter_mut();
                                write_fixnum(entries.len() as i32, buffer);

                                for (key, value) in entries {
                                    if let Some(stripped) = key.strip_prefix("__integer__") {
                                        write_structure(
                                            stripped.parse::<u16>().unwrap().into(),
                                            symbols,
                                            buffer,
                                        );
                                    } else if let Some(stripped) = key.strip_prefix("__object__") {
                                        write_structure(stripped.into(), symbols, buffer)
                                    } else {
                                        write_structure(key.as_str().into(), symbols, buffer);
                                    }

                                    write_structure(value.take(), symbols, buffer);
                                }
                            }
                        }
                    }
                }
                Value::Array(_) => {
                    let array = value.as_array_mut().unwrap();
                    write_byte(Constants::Array, buffer);
                    write_fixnum(array.len() as i32, buffer);

                    for element in array {
                        write_structure(element.take(), symbols, buffer);
                    }
                }
                Value::String(string) => {
                    if string.starts_with("__symbol__") {
                        write_symbol(string.into(), symbols, buffer);
                    } else {
                        write_byte(Constants::InstanceVar, buffer);
                        write_byte(Constants::String, buffer);
                        write_string(string.as_str(), buffer);
                        write_fixnum(1, buffer);
                        write_symbol(ENCODING_SHORT_SYMBOL.into(), symbols, buffer);
                        write_byte(Constants::True, buffer);
                    }
                }
            }
        }
    }
}

/// Serializes JSON object to a Marshal byte stream.
/// /// # Example
/// ```rust
/// use marshal_rs::dump::dump;
/// use serde_json::{Value, json};
///
/// // Value of null
/// let json: serde_json::Value = json!(null); // null
///
/// // Serialize Value to bytes
/// let bytes: Vec<u8> = dump(json);
/// assert_eq!(&bytes, &[0x04, 0x08, 0x30]);
/// ```
pub fn dump(value: Value) -> Vec<u8> {
    let mut buffer: Vec<u8> = Vec::with_capacity(128);
    let mut symbols: Vec<Value> = Vec::new();

    write_buffer(&MARSHAL_VERSION.to_be_bytes(), &mut buffer);
    write_structure(value, &mut symbols, &mut buffer);

    buffer
}
