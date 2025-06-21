//! Utilities for serializing Marshal byte streams to JSON.

use crate::{constants::*, types::*, uuid_json, value_rc};
use decimal::d128;
use encoding_rs::{Encoding, UTF_8};
use num_bigint::BigInt;
use serde_json::{from_value, to_string, Value};
use std::{mem::transmute, rc::Rc, str::FromStr};
use strum_macros::EnumIs;
use thiserror::Error;

#[derive(PartialEq, Clone, Copy, EnumIs)]
pub enum StringMode {
    Auto,
    UTF8,
    Binary,
}

#[derive(Debug, Error)]
pub enum LoadError {
    #[error(
        "Unexpected end of file encountered when expected more bytes. File is probably corrupted."
    )]
    UnexpectedEOF,
    #[error(
        "Invalid starting Marshal version bytes. Expected 4.8 Marshal version. File is probably of incompatible Marshal version or not Marshal."
    )]
    InvalidMarshalVersion,
}

/// Struct for serializing Marshal byte streams to JSON.
pub struct Loader<'a> {
    buffer: &'a [u8],
    byte_position: usize,
    symbols: Vec<ValueRc>,
    objects: Vec<ValueRc>,
    instance_var_prefix: Option<&'a str>,
    string_mode: StringMode,
}

impl<'a> Loader<'a> {
    pub fn new() -> Self {
        Self {
            buffer: &[],
            byte_position: 0,
            symbols: Vec::with_capacity(256),
            objects: Vec::with_capacity(256),
            instance_var_prefix: None,
            string_mode: StringMode::Auto,
        }
    }

    #[inline]
    pub fn set_string_mode(&mut self, mode: StringMode) {
        self.string_mode = mode;
    }

    #[inline]
    pub fn set_instance_var_prefix(&mut self, prefix: &'a str) {
        self.instance_var_prefix = Some(prefix);
    }

    #[inline]
    fn read_byte(&mut self) -> Result<u8, LoadError> {
        let Some(&byte) = self.buffer.get(self.byte_position) else {
            return Err(LoadError::UnexpectedEOF);
        };

        self.byte_position += 1;
        Ok(byte)
    }

    #[inline]
    fn read_bytes(&mut self, amount: usize) -> Result<&[u8], LoadError> {
        let Some(bytes) = self
            .buffer
            .get(self.byte_position..self.byte_position + amount)
        else {
            return Err(LoadError::UnexpectedEOF);
        };

        self.byte_position += amount;
        Ok(bytes)
    }

    #[inline]
    fn read_int(&mut self) -> Result<i32, LoadError> {
        let fixnum_size: i8 = self.read_byte()? as i8;

        Ok(match fixnum_size {
            // Fixnum is zero
            0 => 0,
            // These values mark the length of fixnum in bytes
            -4..=4 => {
                let size = fixnum_size.unsigned_abs() as usize;
                let bytes: &[u8] = self.read_bytes(size)?;
                let mut buffer: [u8; 4] = [if fixnum_size < 0 { 255u8 } else { 0u8 }; 4];

                buffer[..size].copy_from_slice(&bytes[..size]);
                i32::from_le_bytes(buffer)
            }
            // Otherwise fixnum is a single byte and we read it
            _ => {
                if fixnum_size > 0 {
                    fixnum_size as i32 - NUMBER_PADDING
                } else {
                    fixnum_size as i32 + NUMBER_PADDING
                }
            }
        })
    }

    #[inline]
    fn read_chunk(&mut self) -> Result<&[u8], LoadError> {
        let chunk_size = self.read_int()?;
        self.read_bytes(chunk_size as usize)
    }

    #[inline]
    fn read_string(&mut self) -> Result<String, LoadError> {
        let string_bytes: &[u8] = self.read_chunk()?;
        Ok(String::from_utf8_lossy(string_bytes).into_owned())
    }

    #[inline]
    fn read_symbol_link(&mut self) -> Result<ValueRc, LoadError> {
        let symbol_link_pos = self.read_int()?;
        Ok(self.symbols[symbol_link_pos as usize].clone())
    }

    #[inline]
    fn read_object_link(&mut self) -> Result<ValueRc, LoadError> {
        let object_link_pos = self.read_int()?;
        Ok(self.objects[object_link_pos as usize].clone())
    }

    #[inline]
    fn read_symbol(&mut self) -> Result<ValueRc, LoadError> {
        let symbol_string: String = self.read_string()?;
        let symbol: String = format!("{SYMBOL_PREFIX}{symbol_string}");

        let symbol_rc = value_rc!(uuid_json!(symbol));
        self.symbols.push(symbol_rc.clone());
        Ok(symbol_rc)
    }

    #[inline]
    fn read_instance_var(&mut self) -> Result<ValueRc, LoadError> {
        let object_rc = self.read_next()?;
        let object_size = self.read_int()?;

        let object = object_rc.get();
        let mut decode_string = false;

        if !self.string_mode.is_binary() && object["__type"].as_str() == Some("bytes") {
            decode_string = true;
        }

        for _ in 0..object_size {
            let key_rc = self.read_next()?;
            let value_rc = self.read_next()?;

            if !decode_string {
                continue;
            }

            let key = key_rc.get();
            let string_bytes: Vec<u8> = from_value(object["data"].clone().into_value()).unwrap();

            if **key == UTF8_ENCODING_SYMBOL {
                *object = unsafe { uuid_json!(std::str::from_utf8_unchecked(&string_bytes)) };
            } else if **key == NON_UTF8_ENCODING_SYMBOL {
                let value = value_rc.get();
                let encoding_label: Vec<u8> =
                    from_value(value.get_mut("data").unwrap().take().into_value()).unwrap();

                let (cow, _, _) = Encoding::for_label(&encoding_label)
                    .unwrap_or(UTF_8)
                    .decode(&string_bytes);
                *object = uuid_json!(cow.into_owned());

                *self.objects.last_mut().unwrap() = object_rc.clone();
            }
        }

        Ok(object_rc)
    }

    #[inline]
    fn read_extended_object(&mut self) -> Result<ValueRc, LoadError> {
        let symbol_rc = self.read_next()?;
        let object_rc = self.read_next()?;
        let object = object_rc.get();

        if object.is_object() && object.get(EXTENDS_SYMBOL).is_none() {
            object[EXTENDS_SYMBOL] = uuid_json!([symbol_rc.get().take()]);
        }

        Ok(object_rc)
    }

    #[inline]
    fn read_array(&mut self) -> Result<ValueRc, LoadError> {
        let array_size = self.read_int()?;
        let array_rc = value_rc!(uuid_json!(vec![0; array_size as usize]));
        self.objects.push(array_rc.clone());

        let array = array_rc.get();

        for i in 0..array_size as usize {
            let element_rc = self.read_next()?;
            let element = element_rc.get();

            array[i] = element.clone();
        }

        Ok(array_rc)
    }

    #[inline]
    fn read_bigint(&mut self) -> Result<ValueRc, LoadError> {
        let bigint_sign = self.read_byte()?;
        let bigint_size = self.read_int()? << 1;
        let bigint_bytes: &[u8] = self.read_bytes(bigint_size as usize)?;
        let bignum: BigInt = BigInt::from_bytes_le(
            if bigint_sign == Constants::SignPositive {
                num_bigint::Sign::Plus
            } else {
                num_bigint::Sign::Minus
            },
            bigint_bytes,
        );

        let bigint_object = uuid_json!({"__type": "bigint", "value": bignum.to_string()});

        let bigint_rc = value_rc!(bigint_object);
        self.objects.push(bigint_rc.clone());
        Ok(bigint_rc)
    }

    #[inline]
    fn read_class(&mut self) -> Result<ValueRc, LoadError> {
        let class_class = self.read_string()?;
        let class_rc = value_rc!(uuid_json!({ "__class": class_class, "__type": "class" }));
        self.objects.push(class_rc.clone());
        Ok(class_rc)
    }

    #[inline]
    fn read_module(&mut self, is_old: bool) -> Result<ValueRc, LoadError> {
        let module_class = self.read_string()?;
        let module_rc =
            value_rc!(uuid_json!({ "__class": module_class, "__type": "module", "__old": is_old }));
        self.objects.push(module_rc.clone());
        Ok(module_rc)
    }

    #[inline]
    fn read_float(&mut self) -> Result<ValueRc, LoadError> {
        let float_string: &str = &self.read_string()?;

        let float = match float_string {
            "inf" => Some(d128::infinity()),
            "-inf" => Some(d128::neg_infinity()),
            "nan" => None,
            _ => {
                let mut float_chars: std::str::Chars = float_string.chars();
                let first_char: Option<char> = float_chars.next();

                let mut float_string = String::new();

                if let Some(first_char) = first_char {
                    if first_char.is_numeric() || first_char == '-' {
                        float_string.push(first_char);

                        float_string += &float_chars
                            .take_while(|&ch| ch == '.' || ch.is_numeric())
                            .collect::<String>();

                        let float = d128::from_str(&float_string).unwrap_or(d128::zero());
                        Some(float)
                    } else {
                        None
                    }
                } else {
                    None
                }
            }
        };

        let float_rc = value_rc!(match float {
            Some(value) => {
                let json = uuid_json!(format!("{FLOAT_PREFIX}{value}"));
                json
            }
            None => uuid_json!(null),
        });

        self.objects.push(float_rc.clone());
        Ok(float_rc)
    }

    #[inline]
    fn read_hashmap(&mut self, has_default_value: bool) -> Result<ValueRc, LoadError> {
        let hashmap_size = self.read_int()?;
        let hashmap_rc = value_rc!(uuid_json!(serde_json::Map::with_capacity(
            hashmap_size as usize
        )));
        self.objects.push(hashmap_rc.clone());

        let hashmap = hashmap_rc.get();

        for _ in 0..hashmap_size {
            let key_rc = self.read_next()?;
            let value_rc = self.read_next()?;

            let key = key_rc.get();
            let value = value_rc.get();

            let key_prefix = match &**key {
                Value::Null => NULL_PREFIX,
                Value::Bool(_) => BOOLEAN_PREFIX,
                Value::Number(number) => {
                    if number.is_i64() {
                        INTEGER_PREFIX
                    } else {
                        FLOAT_PREFIX
                    }
                }
                Value::String(_) => "",
                Value::Array(_) => ARRAY_PREFIX,
                Value::Object(_) => OBJECT_PREFIX,
            };

            let key = if key_prefix.is_empty() {
                key.as_str().unwrap().to_owned()
            } else {
                format!(
                    "{key_prefix}{}",
                    to_string(&key.clone().into_value()).unwrap()
                )
            };

            hashmap[key.as_str()] = value.take();
        }

        if has_default_value {
            let default_value_rc = self.read_next()?;
            let default_value = default_value_rc.get();

            hashmap[DEFAULT_SYMBOL] = default_value.take();
        }

        Ok(hashmap_rc)
    }

    #[inline]
    fn read_object(&mut self) -> Result<ValueRc, LoadError> {
        let object_class = self.read_next()?;
        let object_rc =
            value_rc!(uuid_json!({ "__class": object_class.get().clone(), "__type": "object" }));
        self.objects.push(object_rc.clone());

        let object_size = self.read_int()?;
        let object = object_rc.get();

        for _ in 0..object_size {
            let key_rc = self.read_next()?;
            let value_rc = self.read_next()?;

            let key = key_rc.get();
            let value = value_rc.get();

            let mut key_string = key.as_str().unwrap().to_string();

            if let Some(prefix) = self.instance_var_prefix {
                key_string.replace_range(SYMBOL_PREFIX.len()..SYMBOL_PREFIX.len() + 1, prefix);
            }

            object[key_string.as_str()] = value.clone();
        }

        Ok(object_rc)
    }

    #[inline]
    fn read_regexp(&mut self) -> Result<ValueRc, LoadError> {
        let regexp_expression = self.read_string()?;
        let regexp_flags = self.read_byte()?;
        let mut regexp_flags_string = String::with_capacity(3);

        if regexp_flags & Constants::RegexpIgnore != 0 {
            regexp_flags_string.push('i');
        }

        if regexp_flags & Constants::RegexpExtended != 0 {
            regexp_flags_string.push('x');
        }

        if regexp_flags & Constants::RegexpMultiline != 0 {
            regexp_flags_string.push('m');
        }

        let regexp = uuid_json!({"__type": "regexp", "expression": regexp_expression, "flags": regexp_flags_string});

        let regexp_rc = value_rc!(regexp);
        self.objects.push(regexp_rc.clone());
        Ok(regexp_rc)
    }

    #[inline]
    fn read_string_object(&mut self) -> Result<ValueRc, LoadError> {
        let string_mode = self.string_mode;
        let string_bytes = self.read_chunk()?;

        let object = if string_mode.is_utf_8() {
            if let Ok(string) = std::str::from_utf8(string_bytes) {
                uuid_json!(string)
            } else {
                uuid_json!({ "__type": "bytes", "data": string_bytes })
            }
        } else {
            uuid_json!({ "__type": "bytes", "data": string_bytes })
        };

        let string_rc = value_rc!(object);
        self.objects.push(string_rc.clone());
        Ok(string_rc)
    }

    #[inline]
    fn read_struct(&mut self) -> Result<ValueRc, LoadError> {
        let struct_class = self.read_next()?;
        let struct_rc =
            value_rc!(uuid_json!({ "__class": struct_class.get(), "__type": "struct" }));
        self.objects.push(struct_rc.clone());

        let struct_size = self.read_int()?;
        let mut struct_map = serde_json::Map::with_capacity(struct_size as usize);

        for _ in 0..struct_size {
            let key_rc = self.read_next()?;
            let value_rc = self.read_next()?;

            let key = key_rc.get();
            let value = value_rc.get();

            let key_prefix = match &**key {
                Value::Null => NULL_PREFIX,
                Value::Bool(_) => BOOLEAN_PREFIX,
                Value::Number(number) => {
                    if number.is_i64() {
                        INTEGER_PREFIX
                    } else {
                        FLOAT_PREFIX
                    }
                }
                Value::String(_) => "",
                Value::Array(_) => ARRAY_PREFIX,
                Value::Object(_) => OBJECT_PREFIX,
            };

            let key_string = if key_prefix.is_empty() {
                key.as_str().unwrap().to_owned()
            } else if key.is_array() {
                format!(
                    "{key_prefix}{}",
                    String::from_utf8(from_value(key.take().into_value()).unwrap()).unwrap()
                )
            } else {
                format!("{key_prefix}{}", to_string(&key).unwrap())
            };

            struct_map.insert(key_string, value.take().into_value());
        }

        struct_rc.get()["__members"] = uuid_json!(Value::Object(struct_map));
        Ok(struct_rc)
    }

    #[inline]
    fn read_data(&mut self, structure_type: Constants) -> Result<ValueRc, LoadError> {
        let data_class = self.read_next()?;
        let data_rc = value_rc!(uuid_json!({ "__class": data_class.get(), "__type": "object" }));
        self.objects.push(data_rc.clone());

        let data = data_rc.get();

        match structure_type {
            Constants::Data => data["__data"] = self.read_next()?.get().take(),
            Constants::UserClass => data["__wrapped"] = self.read_next()?.get().take(),
            Constants::UserDefined => data["__userDefined"] = uuid_json!(self.read_chunk()?),
            Constants::UserMarshal => data["__userMarshal"] = self.read_next()?.get().take(),
            _ => unreachable!(),
        }

        Ok(data_rc)
    }

    #[inline]
    fn read_next(&mut self) -> Result<ValueRc, LoadError> {
        let structure_type: Constants = unsafe { transmute(self.read_byte()?) };

        Ok(match structure_type {
            Constants::Null => value_rc!(uuid_json!(null)),
            Constants::True => value_rc!(uuid_json!(true)),
            Constants::False => value_rc!(uuid_json!(false)),
            Constants::SmallInt => value_rc!(uuid_json!(self.read_int()?)),
            Constants::SymbolLink => self.read_symbol_link()?,
            Constants::ObjectLink => self.read_object_link()?,
            Constants::Symbol => self.read_symbol()?,
            Constants::InstanceVar => self.read_instance_var()?,
            Constants::ExtendedObject => self.read_extended_object()?,
            Constants::Array => self.read_array()?,
            Constants::BigInt => self.read_bigint()?,
            Constants::Class => self.read_class()?,
            Constants::Module | Constants::ModuleOld => {
                self.read_module(structure_type == Constants::ModuleOld)?
            }
            Constants::Float => self.read_float()?,
            Constants::HashMap | Constants::HashMapDefault => {
                self.read_hashmap(structure_type == Constants::HashMapDefault)?
            }
            Constants::Object => self.read_object()?,
            Constants::Regexp => self.read_regexp()?,
            Constants::String => self.read_string_object()?,
            Constants::Struct => self.read_struct()?,
            Constants::Data
            | Constants::UserClass
            | Constants::UserDefined
            | Constants::UserMarshal => self.read_data(structure_type)?,
            _ => unreachable!(),
        })
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
    /// use serde_json::json;
    ///
    /// // Bytes slice of Ruby Marshal data
    /// // Files with Marshal data can be read with std::fs::read()
    /// let bytes: [u8; 3] = [0x04, 0x08, 0x30]; // null
    ///
    /// // Initialize loader
    /// let mut loader = Loader::new();
    ///
    /// // Serialize bytes to a Value
    /// // Returns Result<UuidValue, LoadError>
    /// let json = loader.load(&bytes).unwrap();
    /// assert_eq!(json, json!(null));
    /// ```
    #[inline]
    pub fn load(&mut self, buffer: &[u8]) -> Result<UuidValue, LoadError> {
        self.buffer = unsafe { &*(buffer as *const [u8]) };

        let Some(marshal_version) = self.buffer.get(0..2) else {
            return Err(LoadError::UnexpectedEOF);
        };

        if marshal_version != MARSHAL_VERSION {
            return Err(LoadError::InvalidMarshalVersion);
        }

        self.byte_position += 2;

        let json = self.read_next()?;

        self.symbols.clear();
        self.objects.clear();
        self.byte_position = 0;

        // We just cleared all of the references to this Rc, and can safely unsafely unwrap
        let json = unsafe { Rc::try_unwrap(json).unwrap_unchecked() }.into_inner();
        Ok(json)
    }
}

impl Default for Loader<'_> {
    fn default() -> Self {
        Self::new()
    }
}

/// Serializes Ruby Marshal byte stream to JSON.
///
/// Automatically decides when to convert string to UTF-8. If you want to try to convert all strings to UTF-8, use `load_utf8()`. If you want to keep all strings binary, use `load_binary()`.
///
/// `instance_var_prefix` argument takes an `Option<&str>`, and replaces instance variables' "@" prefixes by this string.
///
/// Returns a `Result<Value, LoadError>`, indicating whether load was successful or not.
/// Returns `Err(InvalidMarshalVersion)`, when passed byte stream is of non-4.8 Marshal version (indicated by two first bytes).
/// Returns `Err(UnexpectedEOF)`, when passed byte stream's data is invalid.
///
/// # Example
/// ```rust
/// use marshal_rs::load;
/// use serde_json::json;
///
/// // Bytes slice of Ruby Marshal data
/// // Files with Marshal data can be read with std::fs::read()
/// let bytes: [u8; 3] = [0x04, 0x08, 0x30]; // null
///
/// // Serialize bytes to a Value
/// // Returns Result<serde_json::Value, LoadError>
/// let json = load(&bytes, None).unwrap();
/// assert_eq!(json, json!(null));
/// ```
#[inline]
pub fn load(buffer: &[u8], instance_var_prefix: Option<&str>) -> Result<UuidValue, LoadError> {
    let mut loader = Loader::new();

    if let Some(prefix) = instance_var_prefix {
        loader.set_instance_var_prefix(prefix);
    }

    loader.load(buffer)
}

/// Serializes Ruby Marshal byte stream to JSON.
///
/// This function tries to convert all encountered strings to UTF-8, and falls back to binary format if it's impossible. If you want to convert only UTF-8 strings to UTF-8, use `load()`. If you want to keep all strings binary, use `load_binary()`.
///
/// `instance_var_prefix` argument takes an `Option<&str>`, and replaces instance variables' "@" prefixes by this string.
///
/// Returns a `Result<Value, LoadError>`, indicating whether load was successful or not.
/// Returns `Err(InvalidMarshalVersion)`, when passed byte stream is of non-4.8 Marshal version (indicated by two first bytes).
/// Returns `Err(UnexpectedEOF)`, when passed byte stream's data is invalid.
///
/// # Example
/// ```rust
/// use marshal_rs::load;
/// use serde_json::json;
///
/// // Bytes slice of Ruby Marshal data
/// // Files with Marshal data can be read with std::fs::read()
/// let bytes: [u8; 3] = [0x04, 0x08, 0x30]; // null
///
/// // Serialize bytes to a Value
/// // Returns Result<UuidValue, LoadError>
/// let json = load(&bytes, None).unwrap();
/// assert_eq!(json, json!(null));
/// ```
#[inline]
pub fn load_utf8(buffer: &[u8], instance_var_prefix: Option<&str>) -> Result<UuidValue, LoadError> {
    let mut loader = Loader::new();
    loader.set_string_mode(StringMode::UTF8);

    if let Some(prefix) = instance_var_prefix {
        loader.set_instance_var_prefix(prefix);
    }

    loader.load(buffer)
}

/// Serializes Ruby Marshal byte stream to JSON.
///
/// This function doesn't try to convert any string to UTF-8, and keeps everything binary. Use if you want to convert only UTF-8 strings to strings, use `load()`. If you want to try to convert all strings to UTF-8, use `load_utf8()`.
///
/// `instance_var_prefix` argument takes an `Option<&str>`, and replaces instance variables' "@" prefixes by this string.
///
/// Returns a `Result<Value, LoadError>`, indicating whether load was successful or not.
/// Returns `Err(InvalidMarshalVersion)`, when passed byte stream is of non-4.8 Marshal version (indicated by two first bytes).
/// Returns `Err(UnexpectedEOF)`, when passed byte stream's data is invalid.
///
/// # Example
/// ```rust
/// use marshal_rs::load;
/// use serde_json::json;
///
/// // Bytes slice of Ruby Marshal data
/// // Files with Marshal data can be read with std::fs::read()
/// let bytes: [u8; 3] = [0x04, 0x08, 0x30]; // null
///
/// // Serialize bytes to a Value
/// // Returns Result<UuidValue, LoadError>
/// let json = load(&bytes, None).unwrap();
/// assert_eq!(json, json!(null));
/// ```
#[inline]
pub fn load_binary(
    buffer: &[u8],
    instance_var_prefix: Option<&str>,
) -> Result<UuidValue, LoadError> {
    let mut loader = Loader::new();
    loader.set_string_mode(StringMode::Binary);

    if let Some(prefix) = instance_var_prefix {
        loader.set_instance_var_prefix(prefix);
    }

    loader.load(buffer)
}
