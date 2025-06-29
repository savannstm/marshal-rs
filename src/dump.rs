//! Utilities for serializing JSON objects back to Marshal byte streams.

use crate::{constants::*, types::*};
use gxhash::{HashMap, HashMapExt};
use num_bigint::{BigInt, Sign};
use std::{mem::take, str::FromStr};

/// Struct for dumping [`Value`] to `Vec<u8>` Marshal data.
///
/// To construct the dumper, use [`Dumper::new`].
///
/// To change instance var prefix, use [`Dumper::set_instance_var_prefix`].
///
/// To dump the data from `Value`, use [`Dumper::dump`].
pub struct Dumper<'a> {
    buffer: Vec<u8>,
    symbols: HashMap<String, usize>,
    objects: HashMap<usize, usize>,
    instance_var_prefix: Option<&'a str>,
}

impl<'a> Dumper<'a> {
    /// Constructs a new dumper with default values.
    ///
    /// To change instance var prefix, use [`Dumper::set_instance_var_prefix`].
    ///
    /// To dump the data from [`Value`], use [`Dumper::dump`].
    pub fn new() -> Self {
        Self {
            buffer: Vec::with_capacity(1024),
            symbols: HashMap::with_capacity(256),
            objects: HashMap::with_capacity(256),
            instance_var_prefix: None,
        }
    }

    /// Sets dumper's instance var prefix to the passed `prefix`.
    pub fn set_instance_var_prefix(&mut self, prefix: &'a str) {
        self.instance_var_prefix = Some(prefix);
    }

    fn remember_object(&mut self, value: &Value) {
        if !self.objects.contains_key(&value.id()) {
            self.objects.insert(value.id(), self.objects.len());
        }
    }

    fn write_byte<T: Into<u8>>(&mut self, byte: T) {
        self.buffer.push(byte.into());
    }

    fn write_buffer(&mut self, buf: &[u8]) {
        self.buffer.extend(buf);
    }

    fn write_bytes(&mut self, buf: &[u8]) {
        self.write_int(buf.len() as i32);
        self.write_buffer(buf);
    }

    fn write_bigint(&mut self, bigint: &str) {
        let bigint = BigInt::from_str(bigint).unwrap();
        let (sign, bytes) = bigint.to_bytes_le();
        let bigint_sign = if sign == Sign::Plus {
            Constants::SignPositive
        } else {
            Constants::SignNegative
        };
        let size_in_u16 = ((bytes.len() + 1) / 2) as u8 + NUMBER_PADDING;

        self.write_byte(Constants::BigInt);
        self.write_byte(bigint_sign);
        self.write_byte(size_in_u16);
        self.write_buffer(&bytes);
    }

    fn write_int(&mut self, number: i32) {
        // In Ruby, 1 bit is reserved for a tag, and is not a part of the actual integer.
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
            1..=122 => buf.push(number as u8 + NUMBER_PADDING),
            -123..=-1 => buf.push(number as u8 - NUMBER_PADDING),
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

    fn write_str(&mut self, string: &str) {
        self.write_bytes(string.as_bytes())
    }

    fn write_symbol(&mut self, symbol: &str) {
        if let Some(&pos) = self.symbols.get(symbol) {
            self.write_byte(Constants::SymbolLink);
            self.write_int(pos as i32);
        } else {
            self.write_byte(Constants::Symbol);
            self.write_str(symbol);
            self.symbols.insert(symbol.to_owned(), self.symbols.len());
        }
    }

    fn write_class_name(
        &mut self,
        data_type: Constants,
        class: &str,
        str: bool,
    ) {
        self.write_byte(data_type);

        if str {
            self.write_str(class);
        } else {
            self.write_symbol(class);
        }
    }

    fn write_instance_var(&mut self, mut object: Object) {
        let object_length: usize = object.len();
        self.write_int(object_length as i32);

        for (key, value) in object.iter_mut() {
            let mut key: String = key.to_owned();

            if let Some(prefix) = self.instance_var_prefix {
                key.replace_range(0..prefix.len(), "@");
            }

            self.write_symbol(key.as_str());
            self.write_structure(value.take());
        }
    }

    fn write_extensions(&mut self, value: &Value) {
        for symbol in value.extensions() {
            self.write_byte(Constants::ExtendedObject);
            self.write_symbol(symbol);
        }
    }

    fn write_regexp(&mut self, regexp_str: &str) {
        let first_slash_pos = regexp_str.find('/').unwrap();
        let last_slash_pos = regexp_str.rfind('/').unwrap();

        let expression_str = &regexp_str[first_slash_pos + 1..last_slash_pos];
        let flags_str = &regexp_str[last_slash_pos + 1..];

        let mut flags: u8 = 0;

        if flags_str.contains('i') {
            flags |= Constants::RegexpIgnore as u8;
        }

        if flags_str.contains('x') {
            flags |= Constants::RegexpExtended as u8;
        }

        if flags_str.contains('m') {
            flags |= Constants::RegexpMultiline as u8;
        }

        self.write_byte(Constants::Regexp);
        self.write_str(expression_str);
        self.write_byte(flags);
    }

    fn write_array(&mut self, array: Vec<Value>) {
        self.write_byte(Constants::Array);
        self.write_int(array.len() as i32);

        for element in array {
            self.write_structure(element);
        }
    }

    fn write_string(&mut self, str: &str) {
        self.write_byte(Constants::InstanceVar);
        self.write_byte(Constants::String);
        self.write_str(str);
        self.write_int(1);
        self.write_symbol(UTF8_ENCODING_SYMBOL);
        self.write_byte(Constants::True);
    }

    fn write_hashmap(
        &mut self,
        mut hashmap: crate::types::HashMap,
        is_struct: bool,
    ) {
        let mut object_len = hashmap.len();

        let default_symbol = Value::symbol(DEFAULT_SYMBOL);
        let default_value = hashmap.get_mut(&default_symbol).map(|x| x.take());

        let hashmap_type = if default_value.is_some() {
            object_len -= 1;
            Constants::HashMapDefault
        } else {
            Constants::HashMap
        };

        if !is_struct {
            self.write_byte(hashmap_type);
        }

        self.write_int(object_len as i32);

        for (key, value) in hashmap.0.into_iter().take(object_len) {
            self.write_structure(key);
            self.write_structure(value);
        }

        if let Some(default_value) = default_value {
            self.write_structure(default_value);
        }
    }

    fn write_structure(&mut self, mut value: Value) {
        let mut class_written: bool = false;

        if value.is_data() {
            self.write_class_name(Constants::Data, value.class_name(), false);
            class_written = true;
        } else if value.is_user_class() {
            self.write_byte(Constants::UserClass);
            self.write_symbol(value.class_name());
            class_written = true;
        } else if value.is_user_defined() {
            let has_instance_var: bool = {
                if let Some(obj) = value.as_object() {
                    !obj.is_empty()
                } else {
                    false
                }
            };

            if has_instance_var {
                self.write_byte(Constants::InstanceVar);
            }

            self.write_class_name(
                Constants::UserDefined,
                value.class_name(),
                false,
            );
            self.write_bytes(value.as_byte_vec().unwrap());

            if has_instance_var {
                self.write_instance_var(value.into_object().unwrap());
            }
            return;
        } else if value.is_user_marshal() {
            self.write_class_name(
                Constants::UserMarshal,
                value.class_name(),
                false,
            );

            class_written = true;
        }

        match *value {
            ValueType::Null => self.write_byte(Constants::Null),
            ValueType::Bool(bool) => {
                let bool_type = if bool {
                    Constants::True
                } else {
                    Constants::False
                };

                self.write_byte(bool_type);
            }
            ValueType::Integer(int) => {
                self.write_byte(Constants::Int);
                self.write_int(int);
            }
            _ => {
                if let Some(&object_pos) = self.objects.get(&value.id()) {
                    self.write_byte(Constants::ObjectLink);
                    self.write_int(object_pos as i32);
                    return;
                }

                self.remember_object(&value);
                self.write_extensions(&value);

                let class =
                    unsafe { &mut *(&mut value as *mut Value) }.class_name();

                match *value {
                    ValueType::Null
                    | ValueType::Bool(_)
                    | ValueType::Integer(_) => unreachable!(),
                    ValueType::Float(ref str) => {
                        self.write_byte(Constants::Float);
                        self.write_str(str);
                    }
                    ValueType::Bigint(ref str) => self.write_bigint(str),
                    ValueType::Symbol(ref str) => self.write_symbol(str),
                    ValueType::String(ref str) => self.write_string(str),
                    ValueType::Regexp(ref str) => self.write_regexp(str),
                    ValueType::Bytes(ref bytes) => {
                        self.write_byte(Constants::String);
                        self.write_bytes(bytes);
                    }
                    ValueType::Array(ref mut array) => {
                        self.write_array(take(array))
                    }
                    ValueType::Object(ref mut object) => {
                        if !class_written {
                            self.write_class_name(
                                Constants::Object,
                                class,
                                false,
                            );
                        }

                        self.write_instance_var(take(object));
                    }
                    ValueType::Class => {
                        if !class_written {
                            self.write_class_name(Constants::Class, class, true)
                        }
                    }
                    ValueType::Module => {
                        self.write_class_name(
                            if value.is_old_module() {
                                Constants::ModuleOld
                            } else {
                                Constants::Module
                            },
                            class,
                            true,
                        );
                    }
                    ValueType::HashMap(ref mut map) => {
                        self.write_hashmap(take(map), false);
                    }
                    ValueType::Struct(ref mut hashmap) => {
                        if !class_written {
                            self.write_class_name(
                                Constants::Struct,
                                class,
                                false,
                            );
                        }

                        self.write_hashmap(take(hashmap), true);
                    }
                }
            }
        }
    }

    /// Serializes JSON object to a Marshal byte stream.
    ///
    /// `instance_var_prefix` argument takes a string, and replaces instance variables' prefixes with Ruby's "@" prefix. It's value must be the same, as in `load()` function.
    /// # Example
    /// ```rust
    /// use marshal_rs::{Dumper, Value};
    ///
    /// let mut dumper = Dumper::new();
    /// let json = Value::null();
    ///
    /// // Serialize Value to Marshal bytes
    /// let bytes: Vec<u8> = dumper.dump(json);
    /// assert_eq!(&bytes, &[0x04, 0x08, 0x30]);
    /// ```
    pub fn dump(&mut self, value: Value) -> Vec<u8> {
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

/// Serializes [`Value`] to a Marshal byte stream.
///
/// `instance_var_prefix` argument takes a string, and replaces instance variables' prefixes with Ruby's "@" prefix. It's value must be the same, as in `load()` function.
///
/// # Example
/// ```rust
/// use marshal_rs::{dump, Value};
///
/// let json = Value::null();
///
/// // Serialize Value to Marshal bytes
/// let bytes: Vec<u8> = dump(json, None);
/// assert_eq!(&bytes, &[0x04, 0x08, 0x30]);
/// ```
pub fn dump(value: Value, instance_var_prefix: Option<&str>) -> Vec<u8> {
    let mut dumper = Dumper::new();

    if let Some(prefix) = instance_var_prefix {
        dumper.set_instance_var_prefix(prefix);
    }

    dumper.dump(value)
}
