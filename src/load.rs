//! Utilities for serializing Marshal byte streams to JSON.

use crate::{
    Constants, DEFAULT_SYMBOL, ENCODING_LONG_SYMBOL, ENCODING_SHORT_SYMBOL, EXTENDS_SYMBOL,
    MARSHAL_VERSION,
};
use encoding_rs::{Encoding, UTF_8};
use num_bigint::BigInt;
#[cfg(not(feature = "sonic"))]
use serde_json::{from_value, json, to_string, Value};
#[cfg(feature = "sonic")]
use sonic_rs::{from_value, json, prelude::*, to_string, Value};
use std::{cell::UnsafeCell, mem::transmute, rc::Rc};

#[derive(PartialEq, Clone, Copy)]
pub enum StringMode {
    UTF8,
    Binary,
}

type ComplexRc = Rc<UnsafeCell<Value>>;

#[derive(Debug)]
pub struct LoadError {
    message: String,
}

impl std::fmt::Display for LoadError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(formatter, "{}", self.message)
    }
}

impl std::error::Error for LoadError {}

pub struct Loader<'a> {
    buffer: &'a [u8],
    byte_position: usize,
    symbols: Vec<ComplexRc>,
    objects: Vec<ComplexRc>,
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
    /// string_mode arguments takes a StringMode enum value, and decodes strings either as binary data or as string objects.
    ///
    /// instance_var_prefix argument takes a string, and replaces instance variables' "@" prefixes by this string.
    ///
    /// Returns a Result, indicating whether load was successful or not.
    /// Returns an Err when:
    /// * Passed byte stream is of non-4.8 Marshal version (indicated by two first bytes).
    /// * Passed byte stream's data is invalid.
    /// # Example
    /// ```rust
    /// use marshal_rs::Loader;
    /// use serde_json::{Value, json};
    ///
    /// // Bytes slice of Ruby Marshal data
    /// // Files with Marshal data can be read with std::fs::read()
    /// let bytes: [u8; 3] = [0x04, 0x08, 0x30]; // null
    ///
    /// // Initialize loader
    /// let mut loader = Loader::new();
    ///
    /// // Serialize bytes to a Value
    /// // If "sonic" feature is enabled, returns Result<sonic_rs::Value, LoadError>, otherwise Result<serde_json::Value, LoadError>
    /// let json: serde_json::Value = loader.load(&bytes, None, None).unwrap();
    /// assert_eq!(json, json!(null));
    /// ```
    pub fn load(
        &mut self,
        buffer: &'a [u8],
        string_mode: Option<StringMode>,
        instance_var_prefix: Option<&'a str>,
    ) -> Result<Value, LoadError> {
        self.buffer = buffer;
        self.string_mode = string_mode;
        self.instance_var_prefix = instance_var_prefix;

        let marshal_version: u16 = u16::from_be_bytes(if let Some(bytes) = self.buffer.get(0..2) {
            bytes.try_into().unwrap()
        } else {
            return Err(LoadError {
                message: "Marshal data is too short. Wasn't even able to read starting version \
                          bytes."
                    .to_string(),
            });
        });

        if marshal_version != MARSHAL_VERSION {
            return Err(LoadError {
                message: "Incompatible Marshal file format or version.".to_string(),
            });
        }

        self.byte_position += 2;

        let read: ComplexRc = self.read_next()?;

        self.symbols.clear();
        self.objects.clear();
        self.byte_position = 0;

        // We just cleared all of the references to this Rc, and can safely unsafely unwrap
        let value: Value = unsafe { Rc::try_unwrap(read).unwrap_unchecked().into_inner() };

        Ok(value)
    }

    fn read_byte(&mut self) -> Result<u8, LoadError> {
        let byte: u8 = if let Some(&byte) = self.buffer.get(self.byte_position) {
            byte
        } else {
            return Err(LoadError {
                message: "Marshal data is too short.".to_string(),
            });
        };

        self.byte_position += 1;
        Ok(byte)
    }

    fn read_bytes(&mut self, amount: usize) -> Result<&[u8], LoadError> {
        let bytes: &[u8] = if let Some(bytes) = self
            .buffer
            .get(self.byte_position..self.byte_position + amount)
        {
            bytes
        } else {
            return Err(LoadError {
                message: format!(
                    "Marshal data is too short. Last position: {}",
                    self.byte_position
                ),
            });
        };

        self.byte_position += amount;
        Ok(bytes)
    }

    fn read_fixnum(&mut self) -> Result<i32, LoadError> {
        let fixnum_length: i8 = self.read_byte()? as i8;

        Ok(match fixnum_length {
            // Fixnum is zero
            0 => 0,
            // These values mark the length of fixnum in bytes
            -4..=4 => {
                let absolute: i8 = fixnum_length.abs();
                let bytes: &[u8] = self.read_bytes(absolute as usize)?;
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
        })
    }

    fn read_chunk(&mut self) -> Result<&[u8], LoadError> {
        let amount: i32 = self.read_fixnum()?;
        self.read_bytes(amount as usize)
    }

    fn read_string(&mut self) -> Result<String, LoadError> {
        let chunk: &[u8] = self.read_chunk()?;
        Ok(String::from_utf8_lossy(chunk).to_string())
    }

    fn read_next(&mut self) -> Result<ComplexRc, LoadError> {
        let structure_type: Constants = unsafe { transmute(self.read_byte()?) };
        Ok(match structure_type {
            Constants::Nil => Rc::from(UnsafeCell::from(json!(null))),
            Constants::True => Rc::from(UnsafeCell::from(Value::from(true))),
            Constants::False => Rc::from(UnsafeCell::from(Value::from(false))),
            Constants::Fixnum => Rc::from(UnsafeCell::from(Value::from(self.read_fixnum()?))),
            Constants::Symlink => {
                let pos: i32 = self.read_fixnum()?;
                Rc::clone(&self.symbols[pos as usize])
            }
            Constants::Link => {
                let pos: i32 = self.read_fixnum()?;
                Rc::clone(&self.objects[pos as usize])
            }
            Constants::Symbol => {
                let prefix: String = String::from("__symbol__");
                let symbol: &String = &self.read_string()?;

                let symbol: Value = ((prefix + symbol).as_str()).into();

                let rc: ComplexRc = Rc::from(UnsafeCell::from(symbol));
                self.symbols.push(rc.clone());
                rc
            }
            Constants::InstanceVar => {
                let object: ComplexRc = self.read_next()?;
                let size: i32 = self.read_fixnum()?;

                unsafe {
                    let object_ptr: &mut Value = &mut *object.get();

                    for _ in 0..size {
                        let key: ComplexRc = self.read_next()?;
                        let mut value: Option<Vec<u8>> = None;

                        let key_ptr: &Value = &*key.get();

                        if let Some(data) = (*self.read_next()?.get()).get_mut("data") {
                            #[cfg(feature = "sonic")]
                            {
                                value = from_value(data).unwrap();
                            }
                            #[cfg(not(feature = "sonic"))]
                            {
                                value = from_value(data.take()).unwrap();
                            }
                        }

                        if (object_ptr["__type"].as_str().unwrap() == "bytes")
                            && [
                                Value::from(ENCODING_LONG_SYMBOL),
                                Value::from(ENCODING_SHORT_SYMBOL),
                            ]
                            .contains(key_ptr)
                            && self.string_mode != Some(StringMode::Binary)
                        {
                            let bytes: Value = object_ptr["data"].clone();
                            let array: Vec<u8>;

                            #[cfg(feature = "sonic")]
                            {
                                array = from_value(&bytes).unwrap()
                            }
                            #[cfg(not(feature = "sonic"))]
                            {
                                array = from_value(bytes).unwrap()
                            }

                            if key_ptr == ENCODING_SHORT_SYMBOL {
                                *object_ptr = (std::str::from_utf8_unchecked(&array)).into();
                            } else {
                                let (cow, _, _) = Encoding::for_label(&value.unwrap())
                                    .unwrap_or(UTF_8)
                                    .decode(&array);

                                #[cfg(feature = "sonic")]
                                {
                                    *object_ptr = cow.into();
                                }
                                #[cfg(not(feature = "sonic"))]
                                {
                                    *object_ptr = (cow.into_owned()).into();
                                }

                                *self.objects.last_mut().unwrap() = object.clone()
                            }
                        }
                    }
                }

                object
            }
            Constants::Extended => {
                let symbol: ComplexRc = self.read_next()?;
                let object: ComplexRc = self.read_next()?;

                unsafe {
                    let object_ref: &mut Value = &mut *object.get();

                    if object_ref.is_object() && object_ref.get(EXTENDS_SYMBOL).is_none() {
                        object_ref[EXTENDS_SYMBOL] = json!([]);
                        object_ref[EXTENDS_SYMBOL]
                            .as_array_mut()
                            .unwrap()
                            .insert(0, (*symbol.get()).clone());
                    }
                }

                object
            }
            Constants::Array => {
                let size: i32 = self.read_fixnum()?;
                let rc: ComplexRc = Rc::from(UnsafeCell::from(json!(vec![0; size as usize])));
                self.objects.push(rc.clone());

                for i in 0..size as usize {
                    unsafe { (*rc.get())[i] = (*self.read_next()?.get()).clone() };
                }

                rc
            }
            Constants::Bignum => {
                let sign: u8 = self.read_byte()?;
                let length: i32 = self.read_fixnum()? << 1;
                let bytes: &[u8] = self.read_bytes(length as usize)?;
                let result: BigInt = BigInt::from_bytes_le(
                    if sign == Constants::Positive {
                        num_bigint::Sign::Plus
                    } else {
                        num_bigint::Sign::Minus
                    },
                    bytes,
                );

                let bignum: Value = json!({"__type": "bigint", "value": result.to_string()});

                let rc: ComplexRc = Rc::from(UnsafeCell::from(bignum));
                self.objects.push(rc.clone());
                rc
            }
            Constants::Class => {
                let rc: ComplexRc = Rc::from(UnsafeCell::from(
                    json!({ "__class": self.read_string()?, "__type": "class" }),
                ));
                self.objects.push(rc.clone());
                rc
            }
            Constants::Module | Constants::ModuleOld => {
                let rc: ComplexRc = Rc::from(UnsafeCell::from(
                    json!({ "__class": self.read_string()?, "__type": "module", "__old": structure_type == Constants::ModuleOld }),
                ));
                self.objects.push(rc.clone());
                rc
            }
            Constants::Float => {
                let string: &str = &self.read_string()?;

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

                                Some(float.parse::<f64>().unwrap_or(0f64))
                            } else {
                                None
                            }
                        } else {
                            None
                        }
                    }
                };

                let object: ComplexRc = Rc::from(UnsafeCell::from(match float {
                    Some(value) => json!(value),
                    None => json!(null),
                }));

                self.objects.push(object.clone());
                object
            }
            Constants::Hash | Constants::HashDefault => {
                let hash_size: i32 = self.read_fixnum()?;
                let rc: ComplexRc = Rc::from(UnsafeCell::from(json!({})));
                self.objects.push(rc.clone());

                for _ in 0..hash_size {
                    let key: ComplexRc = self.read_next()?;
                    let value: ComplexRc = self.read_next()?;

                    unsafe {
                        let key_ptr: &Value = &*key.get();

                        let key: String = if let Some(key) = key_ptr.as_i64() {
                            "__integer__".to_string() + &to_string(&key).unwrap()
                        } else if let Some(key) = key_ptr.as_f64() {
                            "__float__".to_string() + &to_string(&key).unwrap()
                        } else if let Some(key) = key_ptr.as_array() {
                            "__array__".to_string() + &to_string(key).unwrap()
                        } else if let Some(key) = key_ptr.as_object() {
                            "__object__".to_string() + &to_string(&key).unwrap()
                        } else if let Some(key) = key_ptr.as_str() {
                            key.to_string()
                        } else {
                            unreachable!()
                        };

                        (*rc.get())[&key] = (*value.get()).clone();
                    }
                }

                if structure_type == Constants::HashDefault {
                    unsafe { (*rc.get())[DEFAULT_SYMBOL] = (*self.read_next()?.get()).clone() };
                }

                rc
            }
            Constants::Object => {
                let rc: ComplexRc = Rc::from(UnsafeCell::from(
                    json!({ "__class": unsafe { &*self.read_next()?.get() }, "__type": "object" }),
                ));
                self.objects.push(rc.clone());

                let object_size: i32 = self.read_fixnum()?;

                for _ in 0..object_size {
                    let key: &Value = unsafe { &*self.read_next()?.get() };
                    let value: &Value = unsafe { &*self.read_next()?.get() };

                    let mut key_string: String = key.as_str().unwrap().to_string();

                    if let Some(prefix) = self.instance_var_prefix {
                        key_string.replace_range(10..11, prefix);
                    }

                    unsafe {
                        (*rc.get())[key_string.as_str()] = value.clone();
                    }
                }

                rc
            }
            Constants::Regexp => {
                let string: String = self.read_string()?;
                let regex_type: u8 = self.read_byte()?;
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

                let rc: ComplexRc = Rc::from(UnsafeCell::from(regexp));
                self.objects.push(rc.clone());
                rc
            }
            Constants::String => {
                let string_mode: Option<StringMode> = self.string_mode;
                let string_bytes: &[u8] = self.read_chunk()?;

                let object: Value = if string_mode == Some(StringMode::UTF8) {
                    if let Ok(string) = std::str::from_utf8(string_bytes) {
                        string.into()
                    } else {
                        json!({ "__type": "bytes", "data": string_bytes })
                    }
                } else {
                    json!({ "__type": "bytes", "data": string_bytes })
                };

                let rc: ComplexRc = Rc::from(UnsafeCell::from(object));
                self.objects.push(rc.clone());
                rc
            }
            Constants::Struct => {
                let rc: ComplexRc = Rc::from(UnsafeCell::from(
                    json!({ "__class": unsafe { &*self.read_next()?.get() }, "__type": "struct" }),
                ));
                self.objects.push(rc.clone());

                let struct_size: i32 = self.read_fixnum()?;
                let mut hash: Value = json!({});

                for _ in 0..struct_size {
                    let key: Value = unsafe { (*self.read_next()?.get()).clone() };
                    let value: Value = unsafe { (*self.read_next()?.get()).clone() };

                    let mut key_string: String = String::new();

                    if let Some(key_str) = key.as_str() {
                        key_string += key_str;
                    } else if let Some(key_num) = key.as_i64() {
                        key_string += "__integer__";
                        key_string += &key_num.to_string();
                    } else if key.is_array() {
                        let buffer: Vec<u8>;

                        #[cfg(feature = "sonic")]
                        {
                            buffer = from_value(&key).unwrap();
                        }
                        #[cfg(not(feature = "sonic"))]
                        {
                            buffer = from_value(key).unwrap();
                        }

                        key_string = String::from_utf8(buffer).unwrap();
                    } else if let Some(type_) = key["__type"].as_str() {
                        if type_ == "object" {
                            key_string += "__object__";
                            key_string += &to_string(&key).unwrap();
                        }
                    }

                    hash[&key_string] = value;
                }

                unsafe {
                    (*rc.get())["__members"] = hash;
                }
                rc
            }
            Constants::Data
            | Constants::UserClass
            | Constants::UserDefined
            | Constants::UserMarshal => {
                let rc: ComplexRc = Rc::from(UnsafeCell::from(
                    json!({ "__class": unsafe { &*self.read_next()?.get() }, "__type": "object" }),
                ));
                self.objects.push(rc.clone());

                unsafe {
                    let rc_ref: &mut Value = &mut *rc.get();

                    match structure_type {
                        Constants::Data => rc_ref["__data"] = (*self.read_next()?.get()).clone(),
                        Constants::UserClass => {
                            rc_ref["__wrapped"] = (*self.read_next()?.get()).clone()
                        }
                        Constants::UserDefined => {
                            rc_ref["__userDefined"] = (self.read_chunk()?).into()
                        }
                        Constants::UserMarshal => {
                            rc_ref["__userMarshal"] = (*self.read_next()?.get()).clone()
                        }
                        _ => unreachable!(),
                    }
                }

                rc
            }
            _ => unreachable!(),
        })
    }
}

impl<'a> Default for Loader<'a> {
    fn default() -> Self {
        Self::new()
    }
}

/// Serializes Ruby Marshal byte stream to JSON.
///
/// string_mode arguments takes a StringMode enum value, and decodes strings either as binary data or as string objects.
///
/// instance_var_prefix argument takes a string, and replaces instance variables' "@" prefixes by this string.
///
/// Returns a Result, indicating whether load was successful or not.
/// Returns an Err when:
/// * Passed byte stream is of non-4.8 Marshal version (indicated by two first bytes).
/// * Passed byte stream's data is invalid.
/// # Example
/// ```rust
/// use marshal_rs::load;
/// use serde_json::{Value, json};
///
/// // Bytes slice of Ruby Marshal data
/// // Files with Marshal data can be read with std::fs::read()
/// let bytes: [u8; 3] = [0x04, 0x08, 0x30]; // null
///
/// // Serialize bytes to a Value
/// // If "sonic" feature is enabled, returns Result<sonic_rs::Value, LoadError>, otherwise Result<serde_json::Value, LoadError>
/// let json: serde_json::Value = load(&bytes, None, None).unwrap();
/// assert_eq!(json, json!(null));
/// ```
pub fn load(
    buffer: &[u8],
    string_mode: Option<StringMode>,
    instance_var_prefix: Option<&str>,
) -> Result<Value, LoadError> {
    Loader::new().load(buffer, string_mode, instance_var_prefix)
}
