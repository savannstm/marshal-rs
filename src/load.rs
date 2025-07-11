//! Utilities for serializing Marshal byte streams to JSON.

use crate::{VALUE_INSTANCE_COUNTER, constants::*, types::*};
use encoding_rs::{Encoding, UTF_8};
use num_bigint::BigInt;
use std::{mem::transmute, rc::Rc};
use strum_macros::EnumIs;
use thiserror::Error;

type ValueRc = Rc<SafeCell<Value>>;

macro_rules! value_rc {
    ($val:expr) => {
        std::rc::Rc::new(SafeCell::new($val))
    };
}

#[derive(PartialEq, Clone, Copy, EnumIs)]
#[repr(u8)]
pub enum StringMode {
    Auto,
    UTF8,
    Binary,
}

#[derive(Debug, Error)]
#[repr(u8)]
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

/// Struct for serializing Marshal data buffers to [`Value`].
///
/// To construct a loader, use [`Loader::new`].
///
/// To change its string mode or instance var prefix, use [`Loader::set_string_mode`] and [`Loader::set_instance_var_prefix`] respectively.
///
/// To load the data from `&[u8]` buffer, use [`Loader::load`].
pub struct Loader<'a> {
    buffer: &'a [u8],
    byte_position: usize,
    symbols: Vec<ValueRc>,
    objects: Vec<ValueRc>,
    instance_var_prefix: Option<&'a str>,
    string_mode: StringMode,
}

impl<'a> Loader<'a> {
    /// Constructs a new loader with default values.
    ///
    /// To change its string mode or instance var prefix, use [`Loader::set_string_mode`] and [`Loader::set_instance_var_prefix`] respectively.
    ///
    /// To load the data from `&[u8]` buffer, use [`Loader::load`].
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

    /// Sets loader's string mode to the passed `mode`.
    #[inline]
    pub fn set_string_mode(&mut self, mode: StringMode) {
        self.string_mode = mode;
    }

    /// Sets loader's instance var prefix to the passed `prefix`.
    #[inline]
    pub fn set_instance_var_prefix(&mut self, prefix: &'a str) {
        self.instance_var_prefix = Some(prefix);
    }

    #[inline]
    fn read_byte(&mut self) -> Result<u8, LoadError> {
        let byte = if let Some(&byte) = self.buffer.get(self.byte_position) {
            byte
        } else {
            return Err(LoadError::UnexpectedEOF);
        };

        self.byte_position += 1;
        Ok(byte)
    }

    #[inline]
    fn read_bytes(&mut self, amount: usize) -> Result<&[u8], LoadError> {
        let bytes = if let Some(bytes) = self
            .buffer
            .get(self.byte_position..self.byte_position + amount)
        {
            bytes
        } else {
            return Err(LoadError::UnexpectedEOF);
        };

        self.byte_position += amount;
        Ok(bytes)
    }

    #[inline]
    fn read_int(&mut self) -> Result<i32, LoadError> {
        let int_size: i8 = self.read_byte()? as i8;

        Ok(match int_size {
            // Fixnum is zero
            0 => 0,
            // These values mark the length of fixnum in bytes
            -4..=4 => {
                let size = int_size.unsigned_abs() as usize;
                let bytes: &[u8] = self.read_bytes(size)?;
                let mut int_buffer: [u8; 4] =
                    [if int_size < 0 { 255u8 } else { 0u8 }; 4];

                int_buffer[..size].copy_from_slice(&bytes[..size]);
                i32::from_le_bytes(int_buffer)
            }
            // Otherwise fixnum is a single byte and we read it
            _ => {
                if int_size > 0 {
                    int_size as i32 - NUMBER_PADDING as i32
                } else {
                    int_size as i32 + NUMBER_PADDING as i32
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

        let symbol_rc = value_rc!(Value::symbol(symbol_string));
        self.symbols.push(symbol_rc.clone());
        Ok(symbol_rc)
    }

    #[inline]
    fn read_instance_var(&mut self) -> Result<ValueRc, LoadError> {
        let object_rc = self.read_next()?;
        let object_size = self.read_int()?;

        let object = object_rc.get();
        let mut decode_string = false;

        if !self.string_mode.is_binary() && object.is_bytes() {
            decode_string = true;
        }

        for _ in 0..object_size {
            let key_rc = self.read_next()?;
            let value_rc = self.read_next()?;

            if !decode_string {
                continue;
            }

            let key = key_rc.get();
            let string_bytes = object.as_byte_vec().unwrap();

            if let ValueType::Symbol(str) = &**key {
                if str == UTF8_ENCODING_SYMBOL {
                    object.set_value(ValueType::String(unsafe {
                        String::from_utf8_unchecked(string_bytes.to_vec())
                    }));
                } else if str == NON_UTF8_ENCODING_SYMBOL {
                    let value = value_rc.get();
                    let encoding_label = value.as_byte_vec().unwrap();

                    let (cow, _, _) = Encoding::for_label(encoding_label)
                        .unwrap_or(UTF_8)
                        .decode(string_bytes);

                    object.set_value(ValueType::String(cow.into_owned()));
                    *self.objects.last_mut().unwrap() = object_rc.clone();
                }
            }
        }

        Ok(object_rc)
    }

    #[inline]
    fn read_extended_object(&mut self) -> Result<ValueRc, LoadError> {
        let symbol_rc = self.read_next()?;
        let object_rc = self.read_next()?;
        let object = object_rc.get();

        object.add_extension(symbol_rc.get().as_str().unwrap().to_owned());
        Ok(object_rc)
    }

    #[inline]
    fn read_array(&mut self) -> Result<ValueRc, LoadError> {
        let array_size = self.read_int()?;
        let array_rc = value_rc!(Value::null());
        self.objects.push(array_rc.clone());

        let mut array: Vec<Value> = Vec::with_capacity(array_size as usize);

        for _ in 0..array_size as usize {
            let element_rc = self.read_next()?;
            let element = element_rc.get();

            array.push(element.clone());
        }

        *array_rc.get() = Value::array(array);
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

        let bigint_object = Value::bigint(bignum.to_string());

        let bigint_rc = value_rc!(bigint_object);
        self.objects.push(bigint_rc.clone());
        Ok(bigint_rc)
    }

    #[inline]
    fn read_class(&mut self) -> Result<ValueRc, LoadError> {
        let class_class = self.read_string()?;
        let class_rc = value_rc!(Value::class());
        let class = class_rc.get();

        class.set_class(class_class);

        self.objects.push(class_rc.clone());
        Ok(class_rc)
    }

    #[inline]
    fn read_module(&mut self, is_old: bool) -> Result<ValueRc, LoadError> {
        let module_class = self.read_string()?;
        let module_rc = value_rc!(Value::module());
        let module = module_rc.get();

        module.set_old_module(is_old);
        module.set_class(module_class);

        self.objects.push(module_rc.clone());
        Ok(module_rc)
    }

    #[inline]
    fn read_float(&mut self) -> Result<ValueRc, LoadError> {
        let float_string: &str = &self.read_string()?;

        let float = match float_string {
            "inf" | "-inf" | "nan" => float_string,
            _ => {
                let float_string_bytes = float_string.as_bytes();
                let float_end =
                    float_string_bytes.iter().rposition(|x| x.is_ascii_digit());

                if let Some(end) = float_end {
                    &float_string[..=end]
                } else {
                    float_string
                }
            }
        };

        let float_rc = value_rc!(Value::float(float));
        self.objects.push(float_rc.clone());
        Ok(float_rc)
    }

    #[inline]
    fn read_hashmap(
        &mut self,
        has_default_value: bool,
    ) -> Result<ValueRc, LoadError> {
        let hashmap_size = self.read_int()?;
        let hashmap_rc = value_rc!(Value::null());
        self.objects.push(hashmap_rc.clone());

        let mut hashmap = HashMap::with_capacity(hashmap_size as usize);

        for _ in 0..hashmap_size {
            let key_rc = self.read_next()?;
            let value_rc = self.read_next()?;

            let key = key_rc.get();
            let value = value_rc.get();

            hashmap.insert(key.take(), value.take());
        }

        if has_default_value {
            let default_value_rc = self.read_next()?;
            let default_value = default_value_rc.get();

            let default_value_key = Value::symbol(DEFAULT_SYMBOL);
            hashmap.insert(default_value_key, default_value.take());
        }

        hashmap_rc.get().set_value(ValueType::HashMap(hashmap));
        Ok(hashmap_rc)
    }

    #[inline]
    fn read_object(&mut self) -> Result<ValueRc, LoadError> {
        let object_class = self.read_next()?;
        let object_rc = value_rc!(Value::null());
        self.objects.push(object_rc.clone());

        let object_size = self.read_int()?;
        let object = object_rc.get();
        let mut object_map = Object::with_capacity(object_size as usize);

        object.set_class(object_class.get().as_str().unwrap().to_owned());

        for _ in 0..object_size {
            let key_rc = self.read_next()?;
            let value_rc = self.read_next()?;

            let key = key_rc.get();
            let value = value_rc.get();

            let mut key_string = key.as_str().unwrap().to_string();

            if let Some(prefix) = self.instance_var_prefix {
                key_string.replace_range(0..1, prefix);
            }

            object_map.insert(key_string, value.clone());
        }

        object.set_value(ValueType::Object(object_map));
        Ok(object_rc)
    }

    #[inline]
    fn read_regexp(&mut self) -> Result<ValueRc, LoadError> {
        let regexp_expression = self.read_string()?;
        let regexp_flags = self.read_byte()?;

        let mut regexp = format!("/{regexp_expression}/");
        regexp.reserve_exact(3);

        if regexp_flags & Constants::RegexpIgnore != 0 {
            regexp.push('i');
        }

        if regexp_flags & Constants::RegexpExtended != 0 {
            regexp.push('x');
        }

        if regexp_flags & Constants::RegexpMultiline != 0 {
            regexp.push('m');
        }

        let regexp_rc = value_rc!(Value::regexp(regexp));
        self.objects.push(regexp_rc.clone());
        Ok(regexp_rc)
    }

    #[inline]
    fn read_string_object(&mut self) -> Result<ValueRc, LoadError> {
        let string_mode = self.string_mode;
        let string_bytes = self.read_chunk()?;

        let object = if string_mode.is_utf_8() {
            if let Ok(string) = std::str::from_utf8(string_bytes) {
                Value::string(string)
            } else {
                Value::bytes(string_bytes)
            }
        } else {
            Value::bytes(string_bytes)
        };

        let string_rc = value_rc!(object);
        self.objects.push(string_rc.clone());
        Ok(string_rc)
    }

    #[inline]
    fn read_struct(&mut self) -> Result<ValueRc, LoadError> {
        let struct_class = self.read_next()?;
        let struct_rc = value_rc!(Value::null());
        self.objects.push(struct_rc.clone());

        let struct_size = self.read_int()?;
        let mut struct_map = HashMap::with_capacity(struct_size as usize);

        for _ in 0..struct_size {
            let key_rc = self.read_next()?;
            let value_rc = self.read_next()?;

            let key = key_rc.get();
            let value = value_rc.get();

            struct_map.insert(key.take(), value.take());
        }

        let mut struct_object = Value::rstruct(struct_map);
        struct_object
            .set_class(struct_class.get().as_str().unwrap().to_owned());
        *struct_rc.get() = struct_object;

        Ok(struct_rc)
    }

    #[inline]
    fn read_data(
        &mut self,
        structure_type: Constants,
    ) -> Result<ValueRc, LoadError> {
        let data_class = self.read_next()?;
        let data_rc = value_rc!(Value::null());
        self.objects.push(data_rc.clone());

        let data = data_rc.get();

        match structure_type {
            Constants::Data => {
                *data = self.read_next()?.get().take();
                data.set_data(true);
            }
            Constants::UserClass => {
                *data = self.read_next()?.get().take();
                data.set_user_class(true);
            }
            Constants::UserDefined => {
                *data = Value::bytes(self.read_chunk()?);
                data.set_user_defined(true);
            }
            Constants::UserMarshal => {
                *data = self.read_next()?.get().take();
                data.set_user_marshal(true);
            }
            _ => unreachable!(),
        }

        data.set_class(data_class.get().as_str().unwrap().to_owned());
        Ok(data_rc)
    }

    #[inline]
    fn read_next(&mut self) -> Result<ValueRc, LoadError> {
        let structure_type: Constants = unsafe { transmute(self.read_byte()?) };

        Ok(match structure_type {
            Constants::Null => value_rc!(Value::null()),
            Constants::True => value_rc!(Value::bool(true)),
            Constants::False => value_rc!(Value::bool(false)),
            Constants::Int => {
                value_rc!(Value::int(self.read_int()?))
            }
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
    /// use marshal_rs::{Loader, Value};
    ///
    /// // Bytes slice of Ruby Marshal data
    /// // Files with Marshal data can be read with std::fs::read()
    /// let bytes: [u8; 3] = [0x04, 0x08, 0x30]; // null
    ///
    /// // Initialize loader
    /// let mut loader = Loader::new();
    ///
    /// // Serialize bytes to a Value
    /// // Returns Result<Value, LoadError>
    /// let json = loader.load(&bytes).unwrap();
    /// assert_eq!(json, Value::null());
    /// ```
    #[inline]
    pub fn load(&mut self, buffer: &[u8]) -> Result<Value, LoadError> {
        self.buffer = unsafe { &*(buffer as *const [u8]) };

        let marshal_version =
            if let Some(marshal_version) = self.buffer.get(0..2) {
                marshal_version
            } else {
                return Err(LoadError::UnexpectedEOF);
            };

        if marshal_version != MARSHAL_VERSION {
            return Err(LoadError::InvalidMarshalVersion);
        }

        self.byte_position += 2;

        // Reset instance counter
        VALUE_INSTANCE_COUNTER.with(|x| *x.get() = 0);

        let json = self.read_next()?;

        self.symbols.clear();
        self.objects.clear();
        self.byte_position = 0;

        // We just cleared all of the references to this Rc, and can safely unsafely unwrap
        let json =
            unsafe { Rc::try_unwrap(json).unwrap_unchecked() }.into_inner();
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
/// use marshal_rs::{load, Value};
///
/// // Bytes slice of Ruby Marshal data
/// // Files with Marshal data can be read with std::fs::read()
/// let bytes: [u8; 3] = [0x04, 0x08, 0x30]; // null
///
/// // Serialize bytes to a Value
/// // Returns Result<Value, LoadError>
/// let json = load(&bytes, None).unwrap();
/// assert_eq!(json, Value::null());
/// ```
#[inline]
pub fn load(
    buffer: &[u8],
    instance_var_prefix: Option<&str>,
) -> Result<Value, LoadError> {
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
/// use marshal_rs::{load, Value};
///
/// // Bytes slice of Ruby Marshal data
/// // Files with Marshal data can be read with std::fs::read()
/// let bytes: [u8; 3] = [0x04, 0x08, 0x30]; // null
///
/// // Serialize bytes to a Value
/// // Returns Result<Value, LoadError>
/// let json = load(&bytes, None).unwrap();
/// assert_eq!(json, Value::null());
/// ```
#[inline]
pub fn load_utf8(
    buffer: &[u8],
    instance_var_prefix: Option<&str>,
) -> Result<Value, LoadError> {
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
/// use marshal_rs::{load, Value};
///
/// // Bytes slice of Ruby Marshal data
/// // Files with Marshal data can be read with std::fs::read()
/// let bytes: [u8; 3] = [0x04, 0x08, 0x30]; // null
///
/// // Serialize bytes to a Value
/// // Returns Result<Value, LoadError>
/// let json = load(&bytes, None).unwrap();
/// assert_eq!(json, Value::null());
/// ```
#[inline]
pub fn load_binary(
    buffer: &[u8],
    instance_var_prefix: Option<&str>,
) -> Result<Value, LoadError> {
    let mut loader = Loader::new();
    loader.set_string_mode(StringMode::Binary);

    if let Some(prefix) = instance_var_prefix {
        loader.set_instance_var_prefix(prefix);
    }

    loader.load(buffer)
}
