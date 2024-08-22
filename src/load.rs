//! Utilities for serializing Marshal byte streams to JSON.

use crate::{
    Constants, DEFAULT_SYMBOL, ENCODING_LONG_SYMBOL, ENCODING_SHORT_SYMBOL, EXTENDS_SYMBOL,
    MARSHAL_VERSION,
};
use cfg_if::cfg_if;
use core::str;
use encoding_rs::{Encoding, UTF_8};
use num_bigint::BigInt;
use std::{
    cell::{RefCell, UnsafeCell},
    mem::transmute,
    rc::Rc,
};
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
    symbols: Vec<Rc<RefCell<UnsafeCell<Value>>>>,
    objects: Vec<Rc<RefCell<UnsafeCell<Value>>>>,
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

        let value: Value = self.read_next().take().into_inner();

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
                let mut buffer: [u8; 4] = [if fixnum_length < 0 { 255u8 } else { 0u8 }; 4];

                let len: usize = bytes.len().min(4);
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

    fn read_chunk(&mut self) -> &[u8] {
        let amount: i32 = self.read_fixnum();
        self.read_bytes(amount as usize)
    }

    fn read_string(&mut self) -> String {
        String::from_utf8_lossy(self.read_chunk()).to_string()
    }

    fn read_next(&mut self) -> Rc<RefCell<UnsafeCell<Value>>> {
        let structure_type: Constants = unsafe { transmute(self.read_byte()) };
        match structure_type {
            Constants::Nil => Rc::from(RefCell::from(UnsafeCell::from(json!(null)))),
            Constants::True => Rc::from(RefCell::from(UnsafeCell::from(json!(true)))),
            Constants::False => Rc::from(RefCell::from(UnsafeCell::from(json!(false)))),
            Constants::Fixnum => {
                Rc::from(RefCell::from(UnsafeCell::from(json!(self.read_fixnum()))))
            }
            Constants::Symlink => {
                let pos: i32 = self.read_fixnum();
                self.symbols[pos as usize].clone()
            }
            Constants::Link => {
                let pos: i32 = self.read_fixnum();
                self.objects[pos as usize].clone()
            }
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

                let rc = Rc::from(RefCell::from(UnsafeCell::from(symbol)));
                self.symbols.push(rc.clone());
                rc
            }
            Constants::InstanceVar => {
                let object = self.read_next();
                let size: i32 = self.read_fixnum();

                for _ in 0..size {
                    let key = self.read_next();
                    let mut value: Option<Vec<u8>> = None;

                    if let Some(data) = &mut self.read_next().borrow_mut().get_mut().get_mut("data")
                    {
                        cfg_if! {
                            if #[cfg(feature = "sonic")] {
                                value = from_value(data).unwrap();
                            } else {
                                value = from_value(data.take()).unwrap();
                            }
                        }
                    }

                    if (unsafe { &*object.borrow().get() }["__type"]
                        .as_str()
                        .unwrap()
                        == "bytes")
                        && [
                            Value::from(ENCODING_LONG_SYMBOL),
                            Value::from(ENCODING_SHORT_SYMBOL),
                        ]
                        .contains(unsafe { &*key.borrow().get() })
                        && self.string_mode != Some(StringMode::Binary)
                    {
                        let bytes: Value = unsafe { &*object.borrow().get() }["data"].clone();

                        cfg_if! {
                            if #[cfg(feature = "sonic")] {
                                let array: Vec<u8> = from_value(&bytes).unwrap();
                            } else {
                                let array: Vec<u8> = from_value(bytes).unwrap();
                            }
                        }

                        if unsafe { &*key.borrow().get() } == ENCODING_SHORT_SYMBOL {
                            *object.borrow_mut().get_mut() =
                                (unsafe { str::from_utf8_unchecked(&array) }).into();
                        } else {
                            let (cow, _, _) = Encoding::for_label(&value.unwrap())
                                .unwrap_or(UTF_8)
                                .decode(&array);

                            cfg_if! {
                                if #[cfg(feature = "sonic")] {
                                    *object.borrow_mut().get_mut() = cow.into();
                                } else {
                                    *object.borrow_mut().get_mut() = (cow.to_string()).into();
                                }
                            }

                            *self.objects.last_mut().unwrap() = object.clone()
                        }
                    }
                }

                object
            }
            Constants::Extended => {
                let symbol = self.read_next();
                let object = self.read_next();

                if unsafe { &*object.borrow().get() }.is_object()
                    && object.borrow_mut().get_mut().get(EXTENDS_SYMBOL).is_none()
                {
                    object.borrow_mut().get_mut()[EXTENDS_SYMBOL] = json!([]);
                    object.borrow_mut().get_mut()[EXTENDS_SYMBOL]
                        .as_array_mut()
                        .unwrap()
                        .insert(0, symbol.take().into_inner());
                }

                object
            }
            Constants::Array => {
                let size: i32 = self.read_fixnum();
                let rc = Rc::from(RefCell::from(UnsafeCell::from(json!(vec![
                    0;
                    size as usize
                ]))));
                self.objects.push(rc.clone());

                for i in 0..size as usize {
                    rc.borrow_mut().get_mut()[i] =
                        unsafe { &*self.read_next().borrow().get() }.clone();
                }

                rc
            }
            Constants::Bignum => {
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

                let bignum: Value = json!({"__type": "bigint", "value": result.to_string()});

                let rc = Rc::from(RefCell::from(UnsafeCell::from(bignum)));
                self.objects.push(rc.clone());
                rc
            }
            Constants::Class => {
                let object_class: String = self.read_string();

                let rc = Rc::from(RefCell::from(UnsafeCell::from(
                    json!({ "__class": object_class, "__type": "class" }),
                )));
                self.objects.push(rc.clone());
                rc
            }
            Constants::Module | Constants::ModuleOld => {
                let object_class: String = self.read_string();

                let rc = Rc::from(RefCell::from(UnsafeCell::from(
                    json!({ "__class": object_class, "__type": "module", "__old": structure_type == Constants::ModuleOld }),
                )));
                self.objects.push(rc.clone());
                rc
            }
            Constants::Float => {
                let string: &str = &self.read_string();

                let float: Option<f64> = match string {
                    "inf" => Some(f64::INFINITY),
                    "-inf" => Some(-f64::INFINITY),
                    "nan" => None,
                    _ => {
                        let mut chars: std::str::Chars = string.chars();
                        let first_char: Option<char> = chars.next();

                        let mut float: String = String::new();

                        if let Some(first_char) = first_char {
                            if first_char.is_numeric() || first_char == '-' {
                                float.push(first_char);

                                float += &chars
                                    .take_while(|&ch| ch == '.' || ch.is_numeric())
                                    .collect::<String>();

                                Some(float.parse::<f64>().ok().unwrap_or(0f64))
                            } else {
                                None
                            }
                        } else {
                            None
                        }
                    }
                };

                let object = Rc::from(RefCell::from(UnsafeCell::from(match float {
                    Some(value) => json!(value),
                    None => json!(null),
                })));

                self.objects.push(object.clone());
                object
            }
            Constants::Hash | Constants::HashDefault => {
                let hash_size: i32 = self.read_fixnum();
                let rc = Rc::from(RefCell::from(UnsafeCell::from(json!({}))));
                self.objects.push(rc.clone());

                for _ in 0..hash_size {
                    let key = self.read_next();
                    let value = self.read_next();

                    let key: String = if let Some(key) = unsafe { &*key.borrow().get() }.as_i64() {
                        "__integer__".to_string() + &to_string(&key).unwrap()
                    } else if let Some(key) = unsafe { &*key.borrow().get() }.as_f64() {
                        "__float__".to_string() + &to_string(&key).unwrap()
                    } else if let Some(key) = unsafe { &*key.borrow().get() }.as_array() {
                        "__array__".to_string() + &to_string(key).unwrap()
                    } else if let Some(key) = unsafe { &*key.borrow().get() }.as_object() {
                        "__object__".to_string() + &to_string(&key).unwrap()
                    } else if let Some(key) = unsafe { &*key.borrow().get() }.as_str() {
                        key.to_string()
                    } else {
                        panic!()
                    };

                    rc.borrow_mut().get_mut()[&key] = unsafe { &*value.borrow().get() }.clone();
                }

                if structure_type == Constants::HashDefault {
                    rc.borrow_mut().get_mut()[DEFAULT_SYMBOL] =
                        unsafe { &*self.read_next().borrow().get() }.clone();
                }

                rc
            }
            Constants::Object => {
                let object_class = self.read_next();
                let rc = Rc::from(RefCell::from(UnsafeCell::from(
                    json!({ "__class": unsafe { &*object_class.borrow().get() }.clone(), "__type": "object" }),
                )));
                self.objects.push(rc.clone());

                let object_size: i32 = self.read_fixnum();

                for _ in 0..object_size {
                    let key: Value = unsafe { &*self.read_next().borrow().get() }.clone();
                    let value: Value = unsafe { &*self.read_next().borrow().get() }.clone();

                    let mut key_str: String = key.as_str().unwrap().to_string();

                    if let Some(prefix) = self.instance_var_prefix {
                        key_str.replace_range(10..11, prefix)
                    }

                    rc.borrow_mut().get_mut()[key_str.as_str()] = value;
                }

                rc
            }
            Constants::Regexp => {
                let string: String = self.read_string();
                let regex_type: u8 = self.read_byte();
                let mut flags: String = String::new();

                if regex_type & Constants::RegexpIgnore != 0 {
                    flags += "i";
                }

                if regex_type & Constants::RegexpExtended != 0 {
                    flags += "x";
                }

                if regex_type & Constants::RegexpMultiline != 0 {
                    flags += "m";
                }

                let regexp: Value =
                    json!({"__type": "regexp", "expression": string, "flags": flags});

                let rc = Rc::from(RefCell::from(UnsafeCell::from(regexp)));
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

                let rc = Rc::from(RefCell::from(UnsafeCell::from(object)));
                self.objects.push(rc.clone());
                rc
            }
            Constants::Struct => {
                let struct_class = self.read_next();

                let rc = Rc::from(RefCell::from(UnsafeCell::from(
                    json!({ "__class": unsafe {&*struct_class.borrow().get()}, "__type": "struct" }),
                )));
                self.objects.push(rc.clone());

                let struct_size: i32 = self.read_fixnum();
                let mut hash: Value = json!({});

                for _ in 0..struct_size {
                    let key: Value = unsafe { &*self.read_next().borrow().get() }.clone();
                    let value: Value = unsafe { &*self.read_next().borrow().get() }.clone();

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

                rc.borrow_mut().get_mut()["__members"] = hash;
                rc
            }
            Constants::Data
            | Constants::UserClass
            | Constants::UserDefined
            | Constants::UserMarshal => {
                let rc = Rc::from(RefCell::from(UnsafeCell::from(
                    json!({ "__class": unsafe { &*self.read_next().borrow().get() }, "__type": "object" }),
                )));
                self.objects.push(rc.clone());

                match structure_type {
                    Constants::Data => {
                        rc.borrow_mut().get_mut()["__data"] =
                            unsafe { &*self.read_next().borrow().get() }.clone()
                    }
                    Constants::UserClass => {
                        rc.borrow_mut().get_mut()["__wrapped"] =
                            unsafe { &*self.read_next().borrow().get() }.clone()
                    }
                    Constants::UserDefined => {
                        rc.borrow_mut().get_mut()["__userDefined"] = (self.read_chunk()).into()
                    }
                    Constants::UserMarshal => {
                        rc.borrow_mut().get_mut()["__userMarshal"] =
                            unsafe { &*self.read_next().borrow().get() }.clone()
                    }
                    _ => unreachable!(),
                }

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
