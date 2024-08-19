//! Utilities for serializing Marshal byte streams to JSON.

use crate::{
    Constants, DEFAULT_SYMBOL, ENCODING_LONG_SYMBOL, ENCODING_SHORT_SYMBOL, EXTENDS_SYMBOL,
    MARSHAL_VERSION,
};
use cfg_if::cfg_if;
use core::str;
use encoding_rs::{Encoding, UTF_8};
use num_bigint::BigInt;
use std::{cell::RefCell, mem::transmute, rc::Rc};
cfg_if! {
    if #[cfg(feature = "sonic")] {
        use sonic_rs::{
            from_value, json, to_string, JsonContainerTrait, JsonValueMutTrait, JsonValueTrait, Value,
        };
    } else {
        use serde_json::{from_value, json, to_string, Value};
    }
}

#[derive(PartialEq, Clone)]
pub enum StringMode {
    UTF8,
    Binary,
}

pub struct Loader<'a> {
    buffer: &'a [u8],
    byte_position: usize,
    symbols: Vec<Rc<RefCell<Value>>>,
    objects: Vec<Rc<RefCell<Value>>>,
    instance_var_prefix: Option<&'a str>,
    string_mode: Option<StringMode>,
}

impl<'a> Loader<'a> {
    pub fn new() -> Self {
        Self {
            buffer: &[],
            byte_position: 0,
            symbols: Vec::new(),
            objects: Vec::new(),
            instance_var_prefix: None,
            string_mode: None,
        }
    }

    /// Serializes Ruby Marshal byte stream to JSON.
    ///
    /// instance_var_prefix argument takes a string, and replaces instance variables' "@" prefixes by this string.
    /// # Panics
    /// * If passed byte stream is of non-4.8 Marshal version (indicated by two first bytes).
    /// * If passed byte stream's data is invalid.
    /// # Example
    /// ```rust
    /// use marshal_rs::load::Loader;
    /// use serde_json::json;
    ///
    /// // Bytes slice of Ruby Marshal data
    /// // Files with Marshal data can be read with std::fs::read()
    /// let bytes: [u8; 3] = [0x04, 0x08, 0x30]; // null
    ///
    /// // Initialize a loader
    /// let mut loader = Loader::new();
    ///
    /// // Serialize bytes to a Value
    /// // If "sonic" feature is enabled, returns sonic_rs::Value, otherwise serde_json::Value
    /// let json = loader.load(&bytes, None, None);
    /// assert_eq!(json, json!(null));
    /// ```
    pub fn load(
        &mut self,
        buffer: &'a [u8],
        string_mode: Option<StringMode>,
        instance_var_prefix: Option<&'a str>,
    ) -> Value {
        self.buffer = buffer;
        self.string_mode = string_mode;
        self.instance_var_prefix = instance_var_prefix;

        let marshal_version: u16 = u16::from_be_bytes(
            buffer
                .get(0..2)
                .expect("Marshal data is too short.")
                .try_into()
                .unwrap(),
        );

        if marshal_version != MARSHAL_VERSION {
            panic!("Incompatible Marshal file format or version.");
        }

        self.byte_position += 2;

        let value: Value = self.read_next().take();

        self.symbols.clear();
        self.objects.clear();
        self.byte_position = 0;

        value
    }

    fn read_byte(&mut self) -> u8 {
        let byte: u8 = *self
            .buffer
            .get(self.byte_position)
            .expect("Marshal data is too short.");

        self.byte_position += 1;
        byte
    }

    fn read_bytes(&mut self, amount: usize) -> &[u8] {
        let bytes: &[u8] = self
            .buffer
            .get(self.byte_position..self.byte_position + amount)
            .expect("Marshal data is too short.");

        self.byte_position += amount;
        bytes
    }

    fn read_fixnum(&mut self) -> i32 {
        let fixnum_length: i8 = self.read_byte() as i8;

        match fixnum_length {
            // Fixnum is zero
            0 => 0,
            // These values mark the length of fixnum in bytes
            -4..=4 => {
                let absolute: i8 = fixnum_length.abs();
                let bytes: &[u8] = self.read_bytes(absolute as usize);
                let mut buffer = [if fixnum_length < 0 { 255u8 } else { 0u8 }; 4];

                let len = bytes.len().min(4);
                buffer[..len].copy_from_slice(&bytes[..len]);

                i32::from_le_bytes(buffer)
            }
            // Otherwise fixnum is a single byte and we read it
            _ => {
                if fixnum_length > 0 {
                    (fixnum_length - 5) as i32
                } else {
                    (fixnum_length + 5) as i32
                }
            }
        }
    }

    fn get_link_pos(&mut self) -> i32 {
        let fixnum_length: i32 = self.read_byte() as i32;

        match fixnum_length {
            // Fixnum is zero
            0 => 0,
            // These values mark the length of fixnum in bytes
            1..=4 => {
                let absolute: i8 = fixnum_length.abs() as i8;
                let bytes: &[u8] = self.read_bytes(absolute as usize);
                let mut buffer: [u8; 4] = [0; 4];

                let len: usize = bytes.len().min(4);
                buffer[..len].copy_from_slice(&bytes[..len]);

                i32::from_le_bytes(buffer) - 4
            }
            // Otherwise fixnum is a single byte and we read it
            _ => fixnum_length - 5,
        }
    }

    fn read_chunk(&mut self) -> &[u8] {
        let amount: i32 = self.read_fixnum();
        self.read_bytes(amount as usize)
    }

    fn read_string(&mut self) -> String {
        String::from_utf8_lossy(self.read_chunk()).to_string()
    }

    fn read_bignum(&mut self) -> Value {
        let sign: u8 = self.read_byte();
        let length: i32 = self.read_fixnum() << 1;
        let bytes: &[u8] = self.read_bytes(length as usize);
        let result: BigInt = BigInt::from_bytes_le(
            if sign == Constants::Positive {
                num_bigint::Sign::Plus
            } else {
                num_bigint::Sign::Minus
            },
            bytes,
        );

        json!({"__type": "bigint", "value": result.to_string()})
    }

    fn parse_float(&mut self, string: &str) -> Option<f64> {
        let mut chars: std::str::Chars = string.chars();
        let first_char: Option<char> = chars.next();

        let mut float: String = String::new();

        if let Some(first_char) = first_char {
            if first_char.is_numeric() || first_char == '-' {
                float.push(first_char);
            } else {
                return None;
            }
        } else {
            return None;
        }

        float += &chars
            .take_while(|&ch| ch == '.' || ch.is_numeric())
            .collect::<String>();

        float.parse::<f64>().ok()
    }

    fn read_float(&mut self) -> Option<f64> {
        let string: String = self.read_string();

        match string.as_str() {
            "inf" => Some(f64::INFINITY),
            "-inf" => Some(-f64::INFINITY),
            "nan" => None,
            _ => Some(self.parse_float(&string).unwrap_or(0f64)),
        }
    }

    fn read_regexp(&mut self) -> Value {
        let string: String = self.read_string();
        let regex_type: u8 = self.read_byte();
        let mut flags: String = String::new();

        if (regex_type & Constants::RegexpIgnore) != 0 {
            flags += "i";
        }

        if (regex_type & Constants::RegexpMultiline) != 0 {
            flags += "m";
        }

        json!({"__type": "regexp", "expression": string, "flags": flags})
    }

    fn read_next(&mut self) -> Rc<RefCell<Value>> {
        let structure_type: Constants = unsafe { transmute(self.read_byte()) };
        match structure_type {
            Constants::Nil => Rc::from(RefCell::from(json!(null))),
            Constants::True => Rc::from(RefCell::from(json!(true))),
            Constants::False => Rc::from(RefCell::from(json!(false))),
            Constants::Fixnum => Rc::from(RefCell::from(json!(self.read_fixnum()))),
            Constants::Symbol => {
                let prefix: String = String::from("__symbol__");
                let symbol: &String = &self.read_string();

                cfg_if! {
                    if #[cfg(feature = "sonic")] {
                        let symbol: Value = ((prefix + symbol).as_str()).into();
                    } else {
                        let symbol: Value = (prefix + symbol).into();
                    }
                }

                let rc: Rc<RefCell<Value>> = Rc::from(RefCell::from(symbol));
                self.symbols.push(rc.clone());
                rc
            }
            Constants::Symlink => {
                let pos: i32 = self.read_fixnum();
                self.symbols[pos as usize].clone()
            }
            Constants::Link => {
                let pos: i32 = self.get_link_pos();
                self.objects[pos as usize].clone()
            }
            Constants::InstanceVar => {
                let object: Rc<RefCell<Value>> = self.read_next();
                let size: i32 = self.read_fixnum();

                for _ in 0..size {
                    let key: Rc<RefCell<Value>> = self.read_next();
                    let mut value: Option<Vec<u8>> = None;

                    if let Some(data) = self.read_next().borrow_mut().get_mut("data") {
                        cfg_if! {
                            if #[cfg(feature = "sonic")] {
                                value = from_value(data).unwrap();
                            } else {
                                value = from_value(data.take()).unwrap();
                            }
                        }
                    }

                    if (object.borrow()["__type"].as_str().unwrap() == "bytes")
                        && [
                            Value::from(ENCODING_LONG_SYMBOL),
                            Value::from(ENCODING_SHORT_SYMBOL),
                        ]
                        .contains(&key.borrow())
                        && self.string_mode != Some(StringMode::Binary)
                    {
                        let bytes: Value = object.borrow_mut()["data"].take();

                        cfg_if! {
                            if #[cfg(feature = "sonic")] {
                                let array: Vec<u8> = from_value(&bytes).unwrap();
                            } else {
                                let array: Vec<u8> = from_value(bytes).unwrap();
                            }
                        }

                        if *key.borrow() == ENCODING_SHORT_SYMBOL {
                            *object.borrow_mut() =
                                (unsafe { str::from_utf8_unchecked(&array) }).into();
                        } else {
                            let (cow, _, _) = Encoding::for_label(&value.unwrap())
                                .unwrap_or(UTF_8)
                                .decode(&array);

                            cfg_if! {
                                if #[cfg(feature = "sonic")] {
                                    *object.borrow_mut() = cow.into();
                                } else {
                                    *object.borrow_mut() = (cow.to_string()).into();
                                }
                            }

                            *self.objects.last_mut().unwrap() = object.clone()
                        }
                    }
                }

                object
            }
            Constants::Extended => {
                let symbol: Rc<RefCell<Value>> = self.read_next();
                let object: Rc<RefCell<Value>> = self.read_next();

                if object.borrow().is_object() && object.borrow_mut().get(EXTENDS_SYMBOL).is_none()
                {
                    object.borrow_mut()[EXTENDS_SYMBOL] = json!([]);
                    object.borrow_mut()[EXTENDS_SYMBOL]
                        .as_array_mut()
                        .unwrap()
                        .insert(0, symbol.take());
                }

                object
            }
            Constants::Array => {
                let size: i32 = self.read_fixnum();
                let mut array: Value = json!(vec![0; size as usize]);

                for i in 0..size as usize {
                    array[i] = self.read_next().borrow().clone();
                }

                let rc: Rc<RefCell<Value>> = Rc::from(RefCell::from(array));
                self.objects.push(rc.clone());
                rc
            }
            Constants::Bignum => {
                let bignum: Value = self.read_bignum();

                let rc: Rc<RefCell<Value>> = Rc::from(RefCell::from(bignum));
                self.objects.push(rc.clone());
                rc
            }
            Constants::Class => {
                let object_class: String = self.read_string();

                let rc: Rc<RefCell<Value>> = Rc::from(RefCell::from(
                    json!({ "__class": object_class, "__type": "class" }),
                ));
                self.objects.push(rc.clone());
                rc
            }
            Constants::Module | Constants::ModuleOld => {
                let object_class: String = self.read_string();

                let rc: Rc<RefCell<Value>> = Rc::from(RefCell::from(
                    json!({ "__class": object_class, "__type": "module", "__old": structure_type == Constants::ModuleOld }),
                ));
                self.objects.push(rc.clone());
                rc
            }
            Constants::Float => {
                let float: Option<f64> = self.read_float();
                let object = match float {
                    Some(value) => Rc::from(RefCell::from(json!(value))),
                    None => Rc::from(RefCell::from(json!(null))),
                };

                self.objects.push(object.clone());
                object
            }
            Constants::Hash | Constants::HashDefault => {
                let hash_size: i32 = self.read_fixnum();
                let mut hash: Value = json!({});

                for _ in 0..hash_size {
                    let key: Rc<RefCell<Value>> = self.read_next();
                    let value: Rc<RefCell<Value>> = self.read_next();

                    let key: String = if let Some(key) = key.borrow().as_i64() {
                        "__integer__".to_string() + &to_string(&key).unwrap()
                    } else if let Some(key) = key.borrow().as_f64() {
                        "__float__".to_string() + &to_string(&key).unwrap()
                    } else if let Some(key) = key.borrow().as_array() {
                        "__array__".to_string() + &to_string(key).unwrap()
                    } else if let Some(key) = key.borrow().as_object() {
                        "__object__".to_string() + &to_string(&key).unwrap()
                    } else if let Some(key) = key.borrow().as_str() {
                        key.to_string()
                    } else {
                        panic!()
                    };

                    hash[&key] = value.borrow().clone();
                }

                if structure_type == Constants::HashDefault {
                    hash[DEFAULT_SYMBOL] = self.read_next().borrow().clone();
                }

                let rc: Rc<RefCell<Value>> = Rc::from(RefCell::from(hash));
                self.objects.push(rc.clone());
                rc
            }
            Constants::Object => {
                let object_class: Rc<RefCell<Value>> = self.read_next();
                let mut object: Value =
                    json!({ "__class": object_class.borrow().clone(), "__type": "object" });

                let object_size: i32 = self.read_fixnum();

                for _ in 0..object_size {
                    let key: Value = self.read_next().borrow().clone();
                    let value: Value = self.read_next().borrow().clone();

                    let key_str: &str = key.as_str().unwrap();
                    object[key_str
                        .replacen(
                            "__symbol__@",
                            self.instance_var_prefix.unwrap_or("__symbol__@"),
                            1,
                        )
                        .as_str()] = value;
                }

                let rc: Rc<RefCell<Value>> = Rc::from(RefCell::from(object));
                self.objects.push(rc.clone());
                rc
            }
            Constants::Regexp => {
                let regexp: Value = self.read_regexp();

                let rc: Rc<RefCell<Value>> = Rc::from(RefCell::from(regexp));
                self.objects.push(rc.clone());
                rc
            }
            Constants::String => {
                let string_mode: Option<StringMode> = self.string_mode.clone();
                let string_bytes: &[u8] = self.read_chunk();

                let object: Value = if string_mode == Some(StringMode::UTF8) {
                    if let Ok(string) = str::from_utf8(string_bytes) {
                        string.into()
                    } else {
                        json!({ "__type": "bytes", "data": json!(string_bytes) })
                    }
                } else {
                    json!({ "__type": "bytes", "data": json!(string_bytes) })
                };

                let rc: Rc<RefCell<Value>> = Rc::from(RefCell::from(object));
                self.objects.push(rc.clone());
                rc
            }
            Constants::Struct => {
                let struct_class: Rc<RefCell<Value>> = self.read_next();

                cfg_if! {
                    if #[cfg(feature = "sonic")] {
                        let mut ruby_struct: Value =
                            json!({ "__class": struct_class, "__type": "struct" });
                    } else {
                        let mut ruby_struct: Value =
                            json!({ "__class": *struct_class, "__type": "struct" });
                    }
                }

                let struct_size: i32 = self.read_fixnum();
                let mut hash: Value = json!({});

                for _ in 0..struct_size {
                    let key: Value = self.read_next().borrow().clone();
                    let value: Value = self.read_next().borrow().clone();

                    let mut key_string: String = String::new();

                    if let Some(key_str) = key.as_str() {
                        key_string += key_str;
                    } else if let Some(key_num) = key.as_i64() {
                        key_string += "__integer__";
                        key_string += &key_num.to_string();
                    } else if key.is_array() {
                        cfg_if! {
                            if #[cfg(feature = "sonic")] {
                                let buffer: Vec<u8> = from_value(&key).unwrap();
                            } else {
                                let buffer: Vec<u8> = from_value(key).unwrap();
                            }
                        }

                        key_string = String::from_utf8(buffer).unwrap()
                    } else if let Some(type_) = key["__type"].as_str() {
                        if type_ == "object" {
                            key_string += "__object__";
                            key_string += &to_string(&key).unwrap();
                        }
                    }

                    hash[&key_string] = value;
                }

                ruby_struct["__members"] = hash;

                let rc: Rc<RefCell<Value>> = Rc::from(RefCell::from(ruby_struct));
                self.objects.push(rc.clone());
                rc
            }
            Constants::Data
            | Constants::UserClass
            | Constants::UserDefined
            | Constants::UserMarshal => {
                cfg_if! {
                    if #[cfg(feature = "sonic")] {
                        let mut object: Value =
                            json!({ "__class": self.read_next(), "__type": "object" });

                    } else {
                        let mut object: Value =
                            json!({ "__class": *self.read_next(), "__type": "object" });
                    }
                }

                match structure_type {
                    Constants::Data => object["__data"] = self.read_next().borrow().clone(),
                    Constants::UserClass => object["__wrapped"] = self.read_next().borrow().clone(),
                    Constants::UserDefined => object["__userDefined"] = (self.read_chunk()).into(),
                    Constants::UserMarshal => {
                        object["__userMarshal"] = self.read_next().borrow().clone()
                    }
                    _ => unreachable!(),
                }

                let rc: Rc<RefCell<Value>> = Rc::from(RefCell::from(object));
                self.objects.push(rc.clone());
                rc
            }
            _ => unreachable!(),
        }
    }
}

impl<'a> Default for Loader<'a> {
    fn default() -> Self {
        Self::new()
    }
}

/// Serializes Ruby Marshal byte stream to JSON.
///
/// instance_var_prefix argument takes a string, and replaces instance variables' "@" prefixes by this string.
/// # Panics
/// * If passed byte stream is of non-4.8 Marshal version (indicated by two first bytes).
/// * If passed byte stream's data is invalid.
/// # Example
/// ```rust
/// use marshal_rs::load::load;
/// use serde_json::{Value, json};
///
/// // Bytes slice of Ruby Marshal data
/// // Files with Marshal data can be read with std::fs::read()
/// let bytes: [u8; 3] = [0x04, 0x08, 0x30]; // null
///
/// // Serialize bytes to a Value
/// // If "sonic" feature is enabled, returns sonic_rs::Value, otherwise serde_json::Value
/// let json: serde_json::Value = load(&bytes, None, None);
/// assert_eq!(json, json!(null));
/// ```
pub fn load(
    buffer: &[u8],
    string_mode: Option<StringMode>,
    instance_var_prefix: Option<&str>,
) -> Value {
    Loader::new().load(buffer, string_mode, instance_var_prefix)
}
