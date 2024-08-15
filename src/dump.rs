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

pub struct Dumper<'a> {
    buffer: Vec<u8>,
    symbols: Vec<Value>,
    instance_var_prefix: Option<&'a str>,
}

impl<'a> Dumper<'a> {
    pub fn new() -> Self {
        Self {
            buffer: Vec::with_capacity(128),
            symbols: Vec::new(),
            instance_var_prefix: None,
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
    /// let bytes: Vec<u8> = dumper.dump(json, None);
    /// assert_eq!(&bytes, &[0x04, 0x08, 0x30]);
    /// ```
    pub fn dump(&mut self, value: Value, instance_var_prefix: Option<&'a str>) -> Vec<u8> {
        self.instance_var_prefix = instance_var_prefix;

        self.write_buffer(&MARSHAL_VERSION.to_be_bytes());
        self.write_structure(value);

        self.symbols.clear();
        self.instance_var_prefix = None;

        mem::take(&mut self.buffer)
    }

    fn write_byte(&mut self, byte: Constants) {
        self.buffer.push(byte as u8);
    }

    fn write_buffer(&mut self, bytes: &[u8]) {
        self.buffer.extend(bytes);
    }

    fn write_bytes(&mut self, bytes: &[u8]) {
        self.write_fixnum(bytes.len() as i32);
        self.write_buffer(bytes);
    }

    fn write_fixnum(&mut self, number: i32) {
        let mut buf: Vec<u8> = vec![0; 5];
        let end: i8 = self.write_marshal_fixnum(number, &mut buf);

        self.write_buffer(&buf[0..end as usize]);
    }

    fn write_marshal_fixnum(&mut self, mut fixnum: i32, buffer: &mut [u8]) -> i8 {
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

    fn write_string(&mut self, string: &str) {
        self.write_bytes(string.as_bytes())
    }

    fn write_float(&mut self, float: f64) {
        let string: String = float.to_string();

        self.write_string(if float.is_infinite() {
            if float.is_sign_positive() {
                "inf"
            } else {
                "-inf"
            }
        } else if float.is_sign_negative() && float == 0f64 {
            "-0"
        } else {
            string.as_str()
        });
    }

    fn write_symbol(&mut self, mut symbol: Value) {
        if let Some(stripped) = symbol.as_str().unwrap().strip_prefix("__symbol__") {
            symbol = stripped.into();
        }

        if let Some(pos) = self.symbols.iter().position(|sym: &Value| sym == &symbol) {
            self.write_byte(Constants::Symlink);
            self.write_fixnum(pos as i32);
        } else {
            self.write_byte(Constants::Symbol);
            self.write_bytes(symbol.as_str().unwrap().as_bytes());
            self.symbols.push(symbol);
        }
    }

    fn write_extended(&mut self, extended: Vec<Value>) {
        for symbol in extended {
            self.write_byte(Constants::Extended);
            self.write_symbol(symbol);
        }
    }

    fn write_class(&mut self, data_type: Constants, object: &mut Value) {
        if !object[EXTENDS_SYMBOL].is_null() {
            cfg_if! {
                if #[cfg(feature = "sonic")] {
                    self.write_extended(
                        from_value(&object[EXTENDS_SYMBOL]).unwrap(),
                    );
                } else {
                    self.write_extended(
                        from_value(object[EXTENDS_SYMBOL].take()).unwrap(),
                    );
                }
            }
        }

        self.write_byte(data_type);
        self.write_symbol(object["__class"].take());
    }

    fn write_user_class(&mut self, object: &mut Value) {
        if !object[EXTENDS_SYMBOL].is_null() {
            cfg_if! {
                if #[cfg(feature = "sonic")] {
                    self.write_extended(
                        from_value(&object[EXTENDS_SYMBOL]).unwrap(),

                    );
                } else {
                    self.write_extended(
                        from_value(object[EXTENDS_SYMBOL].take()).unwrap(),

                    );
                }
            }
        }

        if !object["__wrapped"].is_null() {
            self.write_byte(Constants::UserClass);
            self.write_symbol(object["__class"].take())
        }
    }

    fn write_instance_var(&mut self, mut object: Value) {
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

        let object_length: usize = object.len();

        self.write_fixnum(object_length as i32);

        if object_length > 0 {
            for (key, value) in object.iter_mut() {
                cfg_if! {
                    if #[cfg(feature = "sonic")] {
                        self.write_symbol(key.into());
                    } else {
                        self.write_symbol(Value::from(key.as_str()));
                    }
                }

                self.write_structure(value.take());
            }
        }
    }

    fn write_bignum(&mut self, mut bignum: i64) {
        self.write_byte(Constants::Bignum);
        self.write_byte(if bignum < 0 {
            Constants::Negative
        } else {
            Constants::Positive
        });

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

        self.write_fixnum(buf.len() as i32 >> 1);
        self.write_buffer(&buf);
    }

    fn write_structure(&mut self, mut value: Value) {
        cfg_if! {
            if #[cfg(feature = "sonic")] {
                match value.get_type() {
                    JsonType::Null =>self.write_byte(Constants::Nil),
                    JsonType::Boolean => {
                        self.write_byte(
                            if value.is_true() {
                                Constants::True
                            } else {
                                Constants::False
                            },
                        );
                    }
                    JsonType::Number => {
                        if let Some(integer) = value.as_i64() {
                            if (-0x40_000_000..0x40_000_000).contains(&integer) {
                                self.write_byte(Constants::Fixnum);
                                self.write_fixnum(integer as i32);
                            } else {
                                self.write_bignum(integer);
                            }
                        } else if let Some(float) = value.as_f64() {
                            self.write_byte(Constants::Float);
                            self.write_float(float);
                        }
                    }
                    JsonType::Object => {
                        if let Some(object_type) = value["__type"].as_str() {
                            match object_type {
                                "bytes" => {
                                    let buf: Vec<u8> = from_value(&value["data"]).unwrap();

                                    self.write_byte(Constants::String);
                                    self.write_bytes(&buf);
                                }
                                "object" => {
                                    if value.get("__data").is_some() {
                                        self.write_class(Constants::Data, &mut value);
                                        self.write_structure(value["__data"].take());
                                    } else if value.get("__wrapped").is_some() {
                                        self.write_user_class( &mut value);
                                        self.write_structure(value["__wrapped"].take());
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
                                            self.write_byte(Constants::InstanceVar);
                                        }

                                        self.write_class(Constants::UserDefined, &mut value);
                                        self.write_bytes(
                                            &from_value::<Vec<u8>>(&value["__userDefined"]).unwrap(),
                                        );

                                        if has_instance_var {
                                            self.write_instance_var(value);
                                        }
                                    } else if value.get("__userMarshal").is_some() {
                                        self.write_class(Constants::UserMarshal, &mut value);
                                        self.write_structure(value["__userMarshal"].take());
                                    } else {
                                        self.write_class(Constants::Object, &mut value);
                                        self.write_instance_var(value);
                                    }
                                }
                                "struct" => {
                                    self.write_class(Constants::Struct, &mut value);
                                    self.write_instance_var(value["__members"].take());
                                }
                                "class" => {
                                    self.write_byte(Constants::Class);
                                    self.write_string(value["__name"].take().as_str().unwrap());
                                }
                                "module" => self.write_byte(
                                    if value.get("__old").is_true() {
                                        Constants::ModuleOld
                                    } else {
                                        Constants::Module
                                    },
                                ),
                                "regexp" => {
                                    self.write_byte(Constants::Regexp);
                                    self.write_string(value["expression"].as_str().unwrap());

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

                                    self.write_byte(options);
                                }
                                _ => {
                                    let object: &mut Object = value.as_object_mut().unwrap();

                                    self.write_byte(
                                        if object.get(&DEFAULT_SYMBOL).is_some() {
                                            Constants::HashDefault
                                        } else {
                                            Constants::Hash
                                        },
                                    );

                                    let entries: sonic_rs::value::object::IterMut = object.iter_mut();
                                    self.write_fixnum(entries.len() as i32);

                                    for (key, value) in entries {
                                        let key_value = if let Some(stripped) = key.strip_prefix("__integer__") {
                                            stripped.parse::<u16>().unwrap().into()
                                        } else if let Some(stripped) = key.strip_prefix("__object__") {
                                            stripped.into()
                                        } else {
                                            key.into()
                                        };

                                        self.write_structure(key_value);
                                        self.write_structure(value.take());
                                    }
                                }
                            }
                        }
                    }
                    JsonType::Array => {
                        let array: &mut Array = value.as_array_mut().unwrap();
                        self.write_byte(Constants::Array);
                        self.write_fixnum(array.len() as i32);

                        for element in array {
                            self.write_structure(element.take());
                        }
                    }
                    JsonType::String => {
                        let string: &str = value.as_str().unwrap();

                        if string.starts_with("__symbol__") {
                            self.write_symbol(string.into());
                        } else {
                            self.write_byte(Constants::InstanceVar);
                            self.write_byte(Constants::String);
                            self.write_string(string);
                            self.write_fixnum(1);
                            self.write_symbol(ENCODING_SHORT_SYMBOL.into());
                            self.write_byte(Constants::True);
                        }
                    }
                }
            } else {
                match value {
                    Value::Null =>self.write_byte(Constants::Nil),
                    Value::Bool(bool) => {
                        self.write_byte(
                            if bool {
                                Constants::True
                            } else {
                                Constants::False
                            },

                        );
                    }
                    Value::Number(number) => {
                        if let Some(integer) = number.as_i64() {
                            if (-0x40_000_000..0x40_000_000).contains(&integer) {
                                self.write_byte(Constants::Fixnum);
                                self.write_fixnum(integer as i32);
                            } else {
                                self.write_bignum(integer);
                            }
                        } else if let Some(float) = number.as_f64() {
                            self.write_byte(Constants::Float);
                            self.write_float(float);
                        }
                    }
                    Value::Object(_) => {
                        if let Some(object_type) = value.get("__type") {
                            match object_type.as_str().unwrap() {
                                "bytes" => {
                                    let buf: Vec<u8> = from_value(value["data"].take()).unwrap();

                                    self.write_byte(Constants::String);
                                    self.write_bytes(&buf);
                                }
                                "object" => {
                                    if value.get("__data").is_some() {
                                        self.write_class(Constants::Data, &mut value);
                                        self.write_structure(value["__data"].take());
                                    } else if value.get("__wrapped").is_some() {
                                        self.write_user_class( &mut value);
                                        self.write_structure(value["__wrapped"].take());
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
                                            self.write_byte(Constants::InstanceVar);
                                        }

                                        self.write_class(Constants::UserDefined, &mut value);
                                        self.write_bytes(
                                            &from_value::<Vec<u8>>(value["__userDefined"].take()).unwrap(),

                                        );

                                        if has_instance_var {
                                            self.write_instance_var(value);
                                        }
                                    } else if value.get("__userMarshal").is_some() {
                                        self.write_class(Constants::UserMarshal, &mut value);
                                        self.write_structure(value["__userMarshal"].take());
                                    } else {
                                        self.write_class(Constants::Object, &mut value);
                                        self.write_instance_var(value);
                                    }
                                }
                                "struct" => {
                                    self.write_class(Constants::Struct, &mut value);
                                    self.write_instance_var(value["__members"].take());
                                }
                                "class" => {
                                    self.write_byte(Constants::Class);
                                    self.write_string(value["__name"].take().as_str().unwrap());
                                }
                                "module" =>self.write_byte(
                                    if value.get("__old").is_some_and(|old| old.as_bool().unwrap()) {
                                        Constants::ModuleOld
                                    } else {
                                        Constants::Module
                                    },

                                ),
                                "regexp" => {
                                    self.write_byte(Constants::Regexp);
                                    self.write_string(value["expression"].as_str().unwrap());

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

                                    self.write_byte(options);
                                }
                                _ => {
                                    let object = value.as_object_mut().unwrap();

                                    self.write_byte(
                                        if object.get(DEFAULT_SYMBOL).is_some() {
                                            Constants::HashDefault
                                        } else {
                                            Constants::Hash
                                        },

                                    );

                                    let entries = object.iter_mut();
                                    self.write_fixnum(entries.len() as i32);

                                    for (key, value) in entries {
                                        let key_value = if let Some(stripped) = key.strip_prefix("__integer__") {
                                            stripped.parse::<u16>().unwrap().into()
                                        } else if let Some(stripped) = key.strip_prefix("__object__") {
                                            stripped.into()
                                        } else {
                                            key.as_str().into()
                                        };

                                        self.write_structure(key_value);

                                        self.write_structure(value.take());
                                    }
                                }
                            }
                        }
                    }
                    Value::Array(_) => {
                        let array = value.as_array_mut().unwrap();
                        self.write_byte(Constants::Array);
                        self.write_fixnum(array.len() as i32);

                        for element in array {
                            self.write_structure(element.take());
                        }
                    }
                    Value::String(string) => {
                        if string.starts_with("__symbol__") {
                            self.write_symbol(string.into());
                        } else {
                            self.write_byte(Constants::InstanceVar);
                            self.write_byte(Constants::String);
                            self.write_string(string.as_str());
                            self.write_fixnum(1);
                            self.write_symbol(ENCODING_SHORT_SYMBOL.into());
                            self.write_byte(Constants::True);
                        }
                    }
                }
            }
        }
    }
}

impl<'a> Default for Dumper<'a> {
    fn default() -> Self {
        Self::new()
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
/// let bytes: Vec<u8> = dump(json, None);
/// assert_eq!(&bytes, &[0x04, 0x08, 0x30]);
/// ```
pub fn dump(value: Value, instance_var_prefix: Option<&str>) -> Vec<u8> {
    Dumper::new().dump(value, instance_var_prefix)
}
