//! Utilities for serializing JSON objects back to Marshal byte streams.

use crate::{constants::*, types::*, uuid_json};
use decimal::d128;
use gxhash::{HashMap, HashMapExt};
use num_bigint::{BigInt, Sign};
use serde_json::{from_str, from_value, Value};
use std::{mem::take, str::FromStr};
use uuid::Uuid;

/// Struct for serializing JSON objects to Marshal byte streams.
pub struct Dumper<'a> {
    buffer: Vec<u8>,
    symbols: HashMap<Value, usize>,
    objects: HashMap<Uuid, usize>,
    instance_var_prefix: Option<&'a str>,
}

impl<'a> Dumper<'a> {
    pub fn new() -> Self {
        Self {
            buffer: Vec::with_capacity(1024),
            symbols: HashMap::with_capacity(256),
            objects: HashMap::with_capacity(256),
            instance_var_prefix: None,
        }
    }

    pub fn set_instance_var_prefix(&mut self, prefix: &'a str) {
        self.instance_var_prefix = Some(prefix);
    }

    #[allow(dead_code)]
    fn remember(&mut self, value: &UuidValue) {
        if !self.objects.contains_key(&value.uuid()) {
            self.objects.insert(value.uuid(), self.objects.len());
        }
    }

    fn write_byte(&mut self, byte: u8) {
        self.buffer.push(byte);
    }

    fn write_buffer(&mut self, bytes: &[u8]) {
        self.buffer.extend(bytes);
    }

    fn write_bytes(&mut self, bytes: &[u8]) {
        self.write_number(bytes.len() as i32);
        self.write_buffer(bytes);
    }

    fn write_bigint(&mut self, bigint: BigInt) {
        let (sign, bytes) = bigint.to_bytes_le();
        let bigint_sign = if sign == Sign::Plus {
            Constants::SignPositive
        } else {
            Constants::SignNegative
        };
        let size_in_u16 = ((bytes.len() + 1) / 2) + NUMBER_PADDING as usize;

        self.write_byte(Constants::BigInt as u8);
        self.write_byte(bigint_sign as u8);
        self.write_byte(size_in_u16 as u8);
        self.write_buffer(&bytes);
    }

    fn write_number(&mut self, number: i32) {
        const I32_MAX: i32 = i32::MAX >> 1;
        const I32_MIN: i32 = !(i32::MAX >> 1);

        const I24_MAX: i32 = I32_MAX >> 6;
        const I24_MIN: i32 = !(I32_MAX >> 6);

        const I16_MAX: i32 = I32_MAX >> 14;
        const I16_MIN: i32 = !(I32_MAX >> 14);

        const I8_MAX: i32 = I32_MAX >> 22;
        const I8_MIN: i32 = !(I32_MAX >> 22);

        let mut buf: Vec<u8> = Vec::with_capacity(5);

        match number {
            0 => buf.push(0),
            1..=122 => buf.push(number as u8 + NUMBER_PADDING as u8),
            -123..=-1 => buf.push(number as u8 - NUMBER_PADDING as u8),
            I8_MIN..=I8_MAX => {
                buf.push(1);
                buf.push(number as u8);
            }
            I16_MIN..=I16_MAX => {
                buf.push(if number < 0 { 254 } else { 2 });
                buf.extend(&number.to_le_bytes()[0..2]);
            }
            I24_MIN..=I24_MAX => {
                buf.push(if number < 0 { 253 } else { 3 });
                buf.extend(&number.to_le_bytes()[0..3]);
            }
            I32_MIN..=I32_MAX => {
                buf.push(if number < 0 { 252 } else { 4 });
                buf.extend(&number.to_le_bytes()[0..4]);
            }
            _ => {}
        }

        self.write_buffer(&buf);
    }

    fn write_string(&mut self, string: &str) {
        self.write_bytes(string.as_bytes())
    }

    fn write_float(&mut self, float: d128) {
        self.write_byte(Constants::Float as u8);
        let string: String = float.to_string();

        self.write_string(if float.is_infinite() {
            if float.is_positive() {
                "inf"
            } else {
                "-inf"
            }
        } else if float.is_negative() && float.is_zero() {
            "-0"
        } else {
            string.as_str()
        });
    }

    #[track_caller]
    fn write_symbol(&mut self, mut symbol: UuidValue) {
        if let Some(stripped) = symbol.as_str().unwrap().strip_prefix(SYMBOL_PREFIX) {
            symbol = uuid_json!(stripped);
        }

        let pos = self.symbols.get(&symbol);

        if let Some(&pos) = pos {
            self.write_byte(Constants::SymbolLink as u8);
            self.write_number(pos as i32);
        } else {
            self.write_byte(Constants::Symbol as u8);
            self.write_bytes(symbol.as_str().unwrap().as_bytes());

            self.symbols.insert(symbol.into_value(), self.symbols.len());
        }
    }

    fn write_extended(&mut self, extended: Vec<UuidValue>) {
        for symbol in extended {
            self.write_byte(Constants::ExtendedObject as u8);
            self.write_symbol(symbol);
        }
    }

    fn write_class(&mut self, data_type: Constants, object: &UuidValue) {
        if let Some(extends) = object.get(EXTENDS_SYMBOL) {
            self.write_extended(from_value(extends.clone().into_value()).unwrap());
        }

        self.write_byte(data_type as u8);
        self.write_symbol(object["__class"].clone());
    }

    fn write_user_class(&mut self, object: &UuidValue) {
        if let Some(extends) = object.get(EXTENDS_SYMBOL) {
            self.write_extended(from_value(extends.clone().into_value()).unwrap());
        }

        if object.get("__wrapped").is_some() {
            self.write_byte(Constants::UserClass as u8);
            self.write_symbol(object["__class"].clone())
        }
    }

    fn write_instance_var(&mut self, mut object: UuidValue) {
        let object = object.as_object_mut().unwrap();

        for key in [
            "__class",
            "__type",
            "__data",
            "__wrapped",
            "__userDefined",
            "__userMarshal",
        ] {
            object.shift_remove(key);
        }

        let object_length: usize = object.len();
        self.write_number(object_length as i32);

        for (key, value) in object.iter_mut() {
            let mut key: String = key.to_owned();

            if let Some(prefix) = self.instance_var_prefix {
                key.replace_range(SYMBOL_PREFIX.len()..SYMBOL_PREFIX.len() + prefix.len(), "@");
            }

            self.write_symbol(uuid_json!(key));
            self.write_structure(value.clone());
        }
    }

    fn write_structure(&mut self, mut value: UuidValue) {
        match *value {
            Value::Null => self.write_byte(Constants::Null as u8),
            Value::Bool(bool) => {
                let bool_type = if bool {
                    Constants::True
                } else {
                    Constants::False
                };

                self.write_byte(bool_type as u8);
            }
            Value::Number(_) => {
                if let Some(integer) = value.as_i64() {
                    self.write_byte(Constants::SmallInt as u8);
                    self.write_number(integer as i32);
                } else {
                    self.remember(&value);
                    let float_string = value.as_f64().unwrap().to_string();
                    self.write_float(d128::from_str(&float_string).unwrap());
                }
            }
            _ => {
                if let Some(&object_pos) = self.objects.get(&value.uuid()) {
                    self.write_byte(Constants::ObjectLink as u8);
                    self.write_number(object_pos as i32);
                    return;
                }

                let mut do_not_remember = false;

                if let Some(str) = value.as_str() {
                    for prefix in PREFIXES {
                        if prefix == FLOAT_PREFIX {
                            continue;
                        }

                        if str.starts_with(prefix) {
                            do_not_remember = true;
                        }
                    }
                }

                if !do_not_remember {
                    self.remember(&value);
                }

                match *value {
                    Value::Object(_) => {
                        if let Some(object_type) = value.get("__type") {
                            match object_type.as_str().unwrap() {
                                "object" => {
                                    if value.get("__data").is_some() {
                                        self.write_class(Constants::Data, &value);
                                        self.write_structure(value["__data"].clone());
                                    } else if value.get("__wrapped").is_some() {
                                        self.write_user_class(&value);
                                        self.write_structure(value["__wrapped"].clone());
                                    } else if value.get("__userDefined").is_some() {
                                        let object = value.as_object_mut().unwrap();
                                        let mut object_length: usize = object.len();

                                        for (key, _) in object {
                                            if key.starts_with("__") {
                                                object_length -= 1;
                                            }
                                        }

                                        let has_instance_var: bool = object_length > 0;

                                        if has_instance_var {
                                            self.write_byte(Constants::InstanceVar as u8);
                                        }

                                        self.write_class(Constants::UserDefined, &value);
                                        self.write_bytes(
                                            &from_value::<Vec<u8>>(
                                                value["__userDefined"].take().into_value(),
                                            )
                                            .unwrap(),
                                        );

                                        if has_instance_var {
                                            self.write_instance_var(value);
                                        }
                                    } else if value.get("__userMarshal").is_some() {
                                        self.write_class(Constants::UserMarshal, &value);
                                        self.write_structure(value["__userMarshal"].take());
                                    } else {
                                        self.write_class(Constants::Object, &value);
                                        self.write_instance_var(value);
                                    }
                                }
                                "bytes" => {
                                    let buf: Vec<u8> =
                                        from_value(value["data"].take().into_value()).unwrap();

                                    self.write_byte(Constants::String as u8);
                                    self.write_bytes(&buf);
                                }
                                "struct" => {
                                    self.write_class(Constants::Struct, &value);
                                    self.write_instance_var(value["__members"].take());
                                }
                                "class" => {
                                    self.write_byte(Constants::Class as u8);
                                    self.write_string(value["__name"].as_str().unwrap());
                                }
                                "module" => {
                                    let mut module_type = Constants::Module;

                                    if let Some(is_old) = value["__old"].as_bool() {
                                        if is_old {
                                            module_type = Constants::ModuleOld;
                                        }
                                    }

                                    self.write_byte(module_type as u8);
                                    self.write_string(value["__name"].as_str().unwrap());
                                }
                                "regexp" => {
                                    self.write_byte(Constants::Regexp as u8);
                                    self.write_string(value["expression"].as_str().unwrap());

                                    let flags_string = value["flags"].as_str().unwrap();
                                    let mut flags: u8 = 0;

                                    if flags_string.contains('i') {
                                        flags |= Constants::RegexpIgnore as u8;
                                    }

                                    if flags_string.contains('x') {
                                        flags |= Constants::RegexpExtended as u8;
                                    }

                                    if flags_string.contains('m') {
                                        flags |= Constants::RegexpMultiline as u8;
                                    }

                                    self.write_byte(flags);
                                }
                                "bigint" => {
                                    let bigint =
                                        BigInt::from_str(value["value"].as_str().unwrap()).unwrap();
                                    self.write_bigint(bigint);
                                }
                                _ => unreachable!(),
                            }
                        } else {
                            let object = value.as_object_mut().unwrap();
                            let default_value = object
                                .get_mut(DEFAULT_SYMBOL)
                                .map(|default_value| default_value.take());

                            let hashmap_type = if default_value.is_some() {
                                Constants::HashMapDefault
                            } else {
                                Constants::HashMap
                            };

                            self.write_byte(hashmap_type as u8);

                            for key in [
                                "__class",
                                "__type",
                                "__data",
                                "__wrapped",
                                "__userDefined",
                                "__userMarshal",
                                DEFAULT_SYMBOL,
                            ] {
                                object.shift_remove(key);
                            }

                            self.write_number(object.len() as i32);

                            for (key, value) in object.into_iter() {
                                let mut key_without_prefix = "";

                                for prefix in [
                                    NULL_PREFIX,
                                    BOOLEAN_PREFIX,
                                    INTEGER_PREFIX,
                                    FLOAT_PREFIX,
                                    ARRAY_PREFIX,
                                    OBJECT_PREFIX,
                                ] {
                                    if let Some(without_prefix) = key.strip_prefix(prefix) {
                                        key_without_prefix = without_prefix;
                                        break;
                                    }
                                }

                                let key = if key_without_prefix.is_empty() {
                                    uuid_json!(key)
                                } else {
                                    uuid_json!(from_str::<Value>(key_without_prefix).unwrap())
                                };

                                self.write_structure(key);
                                self.write_structure(value.take());
                            }

                            if let Some(default_value) = default_value {
                                self.write_structure(default_value);
                            }
                        }
                    }
                    Value::Array(_) => {
                        let array = value.as_array_mut().unwrap();

                        self.write_byte(Constants::Array as u8);
                        self.write_number(array.len() as i32);

                        for element in array {
                            self.write_structure(element.take());
                        }
                    }
                    Value::String(_) => {
                        let str = value.as_str().unwrap();

                        if str.starts_with(SYMBOL_PREFIX) {
                            self.write_symbol(value);
                        } else if str.starts_with(FLOAT_PREFIX) {
                            let float_string = str.strip_prefix(FLOAT_PREFIX).unwrap();
                            self.write_float(decimal::d128::from_str(float_string).unwrap())
                        } else {
                            self.write_byte(Constants::InstanceVar as u8);
                            self.write_byte(Constants::String as u8);
                            self.write_string(str);
                            self.write_number(1);
                            self.write_symbol(uuid_json!(UTF8_ENCODING_SYMBOL));
                            self.write_byte(Constants::True as u8);
                        }
                    }
                    _ => {}
                }
            }
        }
    }

    /// Serializes JSON object to a Marshal byte stream.
    ///
    /// `instance_var_prefix` argument takes a string, and replaces instance variables' prefixes with Ruby's "@" prefix. It's value must be the same, as in `load()` function.
    /// # Example
    /// ```rust
    /// use marshal_rs::{Dumper, uuid_json};
    /// use serde_json::json;
    ///
    /// let mut dumper = Dumper::new();
    /// let json = uuid_json!(null);
    ///
    /// // Serialize Value to Marshal bytes
    /// let bytes: Vec<u8> = dumper.dump(json);
    /// assert_eq!(&bytes, &[0x04, 0x08, 0x30]);
    /// ```
    pub fn dump(&mut self, value: UuidValue) -> Vec<u8> {
        self.write_buffer(MARSHAL_VERSION);
        self.write_structure(value);

        self.objects.clear();
        self.symbols.clear();

        take(&mut self.buffer)
    }
}

impl Default for Dumper<'_> {
    fn default() -> Self {
        Self::new()
    }
}

/// Serializes JSON object to a Marshal byte stream.
///
/// `instance_var_prefix` argument takes a string, and replaces instance variables' prefixes with Ruby's "@" prefix. It's value must be the same, as in `load()` function.
/// # Example
/// ```rust
/// use marshal_rs::{dump, uuid_json};
///
/// let json = uuid_json!(null);
///
/// // Serialize Value to bytes
/// let bytes: Vec<u8> = dump(json, None);
/// assert_eq!(&bytes, &[0x04, 0x08, 0x30]);
/// ```
pub fn dump(value: UuidValue, instance_var_prefix: Option<&str>) -> Vec<u8> {
    let mut dumper = Dumper::new();

    if let Some(prefix) = instance_var_prefix {
        dumper.set_instance_var_prefix(prefix);
    }

    dumper.dump(value)
}
