//! Utilities for serializing JSON objects back to Marshal byte streams.

use crate::{Constants, DEFAULT_SYMBOL, ENCODING_SHORT_SYMBOL, EXTENDS_SYMBOL, MARSHAL_VERSION};
use cfg_if::cfg_if;
use num_bigint::{BigInt, Sign};
use std::{mem, str::FromStr};
cfg_if! {
    if #[cfg(feature = "sonic")] {
        use sonic_rs::{
            json, from_value, Array, JsonContainerTrait, JsonType, JsonValueMutTrait, JsonValueTrait, Object,
            Value, from_str
        };
    } else {
        use serde_json::{from_value, Value, from_str};
        use std::collections::HashMap;
    }
}

cfg_if! {
    if #[cfg(feature = "sonic")] {
        pub struct Dumper<'a> {
            buffer: Vec<u8>,
            symbols: Vec<Value>,
            objects: Vec<Value>,
            instance_var_prefix: Option<&'a str>,
        }
    } else {
        pub struct Dumper<'a> {
            buffer: Vec<u8>,
            symbols: HashMap<Value, usize>,
            objects: HashMap<Value, usize>,
            instance_var_prefix: Option<&'a str>,
        }
    }
}

impl<'a> Dumper<'a> {
    pub fn new() -> Self {
        cfg_if! {
            if #[cfg(feature = "sonic")] {
                Self {
                    buffer: Vec::with_capacity(128),
                    symbols: Vec::new(),
                    objects: Vec::new(),
                    instance_var_prefix: None,
                }
            } else {
                Self {
                    buffer: Vec::with_capacity(128),
                    symbols: HashMap::new(),
                    objects: HashMap::new(),
                    instance_var_prefix: None,
                }
            }
        }
    }

    /// Serializes JSON object to a Marshal byte stream.
    ///
    /// instance_var_prefix argument takes a string, and replaces instance variables' prefixes with Ruby's "@" prefix. It's value must be the same, as in load() function.
    /// # Example
    /// ```rust
    /// use marshal_rs::dump::Dumper;
    /// use serde_json::{Value, json};
    ///
    /// // Initialize dumper
    /// let mut dumper = Dumper::new();
    ///
    /// // Value of null
    /// let json = json!(null); // null
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

    fn write_bignum(&mut self, bignum: BigInt) {
        let (sign, mut bytes) = bignum.to_bytes_le();

        self.write_byte(Constants::Bignum as u8);
        self.write_byte(if sign == Sign::Plus {
            Constants::Positive
        } else {
            Constants::Negative
        } as u8);

        bytes[0] = 0;
        bytes.push(0);

        self.write_byte(bytes.len() as u8);
        self.write_buffer(&bytes);
    }

    fn write_number(&mut self, number: i32) {
        let mut buf: Vec<u8> = Vec::with_capacity(5);

        match number {
            0 => buf.push(0),
            1..=122 => buf.push(number as u8 + 5),
            -123..=-1 => buf.push(number as u8 - 5),
            -256..=255 => {
                buf.push(1);
                buf.push(number as u8);
            }
            -65535..=65534 => {
                buf.push(if number < 0 { 254 } else { 2 });
                buf.extend(&(number as i16).to_le_bytes());
            }
            -16777216..=16777215 => {
                buf.push(if number < 0 { 253 } else { 3 });
                buf.extend(&number.to_le_bytes()[0..3]);
            }
            -1073741824..=1073741823 => {
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

        cfg_if! {
            if #[cfg(feature = "sonic")] {
                let pos = self.symbols.iter().position(|sym| *sym == symbol);
            } else {
                let pos = self.symbols.get(&symbol).copied();
            }
        }
        if let Some(pos) = pos {
            self.write_byte(Constants::Symlink as u8);
            self.write_number(pos as i32);
        } else {
            self.write_byte(Constants::Symbol as u8);
            self.write_bytes(symbol.as_str().unwrap().as_bytes());

            cfg_if! {
                if #[cfg(feature = "sonic")] {
                    self.symbols.push(symbol);
                } else {
                    self.symbols.insert(symbol, self.symbols.len());
                }
            }
        }
    }

    fn write_extended(&mut self, extended: Vec<Value>) {
        for symbol in extended {
            self.write_byte(Constants::Extended as u8);
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

        self.write_byte(data_type as u8);
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
            self.write_byte(Constants::UserClass as u8);
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
                    object.shift_remove(key);
                }
            }
        }

        let object_length: usize = object.len();
        self.write_number(object_length as i32);

        if object_length > 0 {
            for (key, value) in object.iter_mut() {
                let key_string: String =
                    key.replacen(self.instance_var_prefix.unwrap_or("@"), "@", 1);

                self.write_symbol(key_string.as_str().into());
                self.write_structure(value.take());
            }
        }
    }

    fn write_structure(&mut self, mut value: Value) {
        cfg_if! {
            if #[cfg(feature = "sonic")] {
                if let Some(value) = self.objects.iter().position(|val| *val == value) {
                    self.write_byte(Constants::Link as u8);
                    self.write_number(value as i32);
                    return;
                }

                match value.get_type() {
                    JsonType::Null => self.write_byte(Constants::Nil as u8),
                    JsonType::Boolean => {
                        self.write_byte(
                            if value.is_true() {
                                Constants::True
                            } else {
                                Constants::False
                            } as u8,
                        );
                    }
                    JsonType::Number => {
                        if let Some(integer) = value.as_i64() {
                            self.write_byte(Constants::Fixnum as u8);
                            self.write_number(integer as i32);
                        } else if let Some(float) = value.as_f64() {
                            if !self.objects.contains(&value) {
                                self.objects.push(value);
                            }

                            self.write_byte(Constants::Float as u8);
                            self.write_float(float);
                        }
                    }
                    JsonType::Object => {
                        if let Some(object_type) = value["__type"].as_str() {
                            match object_type {
                                "bytes" => {
                                    let buf: Vec<u8> = from_value(&value["data"]).unwrap();

                                    if !self.objects.contains(&value["data"]) {
                                        self.objects.push(value["data"].take());
                                    }

                                    self.write_byte(Constants::String as u8);
                                    self.write_bytes(&buf);
                                }
                                "object" => {
                                    if !self.objects.contains(&value) {
                                        self.objects.push(value.clone());
                                    }

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
                                            self.write_byte(Constants::InstanceVar as u8);
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
                                    if !self.objects.contains(&value) {
                                        self.objects.push(value.clone());
                                    }

                                    self.write_class(Constants::Struct, &mut value);
                                    self.write_instance_var(value["__members"].take());
                                }
                                "class" => {
                                    if !self.objects.contains(&value) {
                                        self.objects.push(value.clone());
                                    }

                                    self.write_byte(Constants::Class as u8);
                                    self.write_string(value["__name"].take().as_str().unwrap());
                                }
                                "module" => {
                                    if !self.objects.contains(&value) {
                                        self.objects.push(value.clone());
                                    }

                                    self.write_byte(
                                        if value.get("__old").is_true() {
                                            Constants::ModuleOld
                                        } else {
                                            Constants::Module
                                        } as u8,
                                    );

                                    self.write_string(value["__name"].take().as_str().unwrap());
                                },
                                "regexp" => {
                                    if !self.objects.contains(&value) {
                                        self.objects.push(value.clone());
                                    }

                                    self.write_byte(Constants::Regexp as u8);
                                    self.write_string(value["expression"].as_str().unwrap());

                                    let flags = value["flags"].as_str().unwrap();
                                    let mut options: u8 = 0;

                                    if flags.contains("i") {
                                        options |= Constants::RegexpIgnore as u8;
                                    }

                                    if flags.contains("x") {
                                        options |= Constants::RegexpExtended as u8;
                                    }

                                    if flags.contains("m") {
                                        options |= Constants::RegexpMultiline as u8;
                                    }

                                    self.write_byte(options as u8);
                                }
                                "bigint" => {
                                    if !self.objects.contains(&value) {
                                        self.objects.push(value.clone());
                                    }

                                    let bigint = BigInt::from_str(value["value"].as_str().unwrap()).unwrap();
                                    self.write_bignum(bigint);
                                }
                                _ => unreachable!()
                            }
                        } else {
                            if !self.objects.contains(&value) {
                                self.objects.push(value.clone());
                            }

                            let object: &mut Object = value.as_object_mut().unwrap();
                            let default_value: Option<Value> = object
                                                                    .get_mut(&DEFAULT_SYMBOL)
                                                                    .map(|default_value| default_value.take());

                            let hash_type = if default_value.is_some() {
                                Constants::HashDefault
                            } else {
                                Constants::Hash
                            };

                            self.write_byte(hash_type as u8);

                            for key in [
                                "__class",
                                "__type",
                                "__data",
                                "__wrapped",
                                "__userDefined",
                                "__userMarshal",
                                DEFAULT_SYMBOL,
                            ] {
                                object.remove(&key);
                            }

                            let entries = object.iter_mut();
                            self.write_number(entries.len() as i32);

                            for (key, value) in entries {
                                let key_value = if let Some(stripped) = key.strip_prefix("__integer__") {
                                    stripped.parse::<u64>().unwrap().into()
                                } else if let Some(stripped) = key.strip_prefix("__float__") {
                                    json!(stripped.parse::<f64>().unwrap())
                                } else if let Some(stripped) = key.strip_prefix("__array__") {
                                    from_str(stripped).unwrap()
                                } else if let Some(stripped) = key.strip_prefix("__object__") {
                                    from_str(stripped).unwrap()
                                } else {
                                    key.into()
                                };

                                self.write_structure(key_value);
                                self.write_structure(value.take());
                            }

                            if let Some(default_value) = default_value {
                                self.write_structure(default_value);
                            }
                        }
                    }
                    JsonType::Array => {
                        if !self.objects.contains(&value) {
                            self.objects.push(value.clone());
                        }

                        let array: &mut Array = value.as_array_mut().unwrap();
                        self.write_byte(Constants::Array as u8);
                        self.write_number(array.len() as i32);

                        for element in array {
                            self.write_structure(element.take());
                        }
                    }
                    JsonType::String => {
                        let string: &str = value.as_str().unwrap();

                        if string.starts_with("__symbol__") {
                            self.write_symbol(string.into());
                        } else {
                            if !self.objects.contains(&value) {
                                self.objects.push(value.clone());
                            }

                            self.write_byte(Constants::InstanceVar as u8);
                            self.write_byte(Constants::String as u8);
                            self.write_string(string);
                            self.write_number(1);
                            self.write_symbol(ENCODING_SHORT_SYMBOL.into());
                            self.write_byte(Constants::True as u8);
                        }
                    }
                }
            } else {
                if let Some(&value) = self.objects.get(&value) {
                    self.write_byte(Constants::Link as u8);
                    self.write_number(value as i32);
                    return;
                }

                match value {
                    Value::Null => self.write_byte(Constants::Nil as u8),
                    Value::Bool(bool) => {
                        self.write_byte(
                            if bool {
                                Constants::True
                            } else {
                                Constants::False
                            } as u8,
                        );
                    }
                    Value::Number(_) => {
                        if let Some(integer) = value.as_i64() {
                            self.write_byte(Constants::Fixnum as u8);
                            self.write_number(integer as i32);
                        } else if let Some(float) = value.as_f64() {
                            if !self.objects.contains_key(&value) {
                                self.objects.insert(value, self.objects.len());
                            }

                            self.write_byte(Constants::Float as u8);
                            self.write_float(float);
                        }
                    }
                    Value::Object(_) => {
                        if let Some(object_type) = value.get("__type") {
                            match object_type.as_str().unwrap() {
                                "bytes" => {
                                    let buf: Vec<u8> = from_value(value["data"].clone()).unwrap();

                                    if !self.objects.contains_key(&value["data"]) {
                                        self.objects.insert(value["data"].take(), self.objects.len());
                                    }

                                    self.write_byte(Constants::String as u8);
                                    self.write_bytes(&buf);
                                }
                                "object" => {
                                    if !self.objects.contains_key(&value) {
                                        self.objects.insert(value.clone(), self.objects.len());
                                    }

                                    if value.get("__data").is_some() {
                                        self.write_class(Constants::Data, &mut value);
                                        self.write_structure(value["__data"].take());
                                    } else if value.get("__wrapped").is_some() {
                                        self.write_user_class(&mut value);
                                        self.write_structure(value["__wrapped"].take());
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
                                    if !self.objects.contains_key(&value) {
                                        self.objects.insert(value.clone(), self.objects.len());
                                    }

                                    self.write_class(Constants::Struct, &mut value);
                                    self.write_instance_var(value["__members"].take());
                                }
                                "class" => {
                                    if !self.objects.contains_key(&value) {
                                        self.objects.insert(value.clone(), self.objects.len());
                                    }

                                    self.write_byte(Constants::Class as u8);
                                    self.write_string(value["__name"].take().as_str().unwrap());
                                }
                                "module" => {
                                    if !self.objects.contains_key(&value) {
                                        self.objects.insert(value.clone(), self.objects.len());
                                    }

                                    self.write_byte(
                                        if let Some(old) = value.get("__old") {
                                            if old.as_bool().unwrap() {
                                                Constants::ModuleOld
                                            } else {
                                                Constants::Module
                                            }
                                        } else {
                                            Constants::Module
                                        } as u8
                                    );

                                    self.write_string(value["__name"].take().as_str().unwrap());
                                },
                                "regexp" => {
                                    if !self.objects.contains_key(&value) {
                                        self.objects.insert(value.clone(), self.objects.len());
                                    }

                                    self.write_byte(Constants::Regexp as u8);
                                    self.write_string(value["expression"].as_str().unwrap());

                                    let flags = value["flags"].as_str().unwrap();
                                    let mut options: u8 = 0;

                                    if flags.contains("i") {
                                        options |= Constants::RegexpIgnore as u8;
                                    }

                                    if flags.contains("x") {
                                        options |= Constants::RegexpExtended as u8;
                                    }

                                    if flags.contains("m") {
                                        options |= Constants::RegexpMultiline as u8;
                                    }

                                    self.write_byte(options);
                                }
                                "bigint" => {
                                    if !self.objects.contains_key(&value) {
                                        self.objects.insert(value.clone(), self.objects.len());
                                    }

                                    let bigint = BigInt::from_str(value["value"].as_str().unwrap()).unwrap();
                                    self.write_bignum(bigint);
                                }
                                _ => unreachable!()
                            }
                        } else {
                            if !self.objects.contains_key(&value) {
                                self.objects.insert(value.clone(), self.objects.len());
                            }

                            let object = value.as_object_mut().unwrap();
                            let default_value: Option<Value> = object
                                                                    .get_mut(DEFAULT_SYMBOL)
                                                                    .map(|default_value| default_value.take());

                            let hash_type = if default_value.is_some() {
                                Constants::HashDefault
                            } else {
                                Constants::Hash
                            };

                            self.write_byte(hash_type as u8);

                            for key in [
                                "__class",
                                "__type",
                                "__data",
                                "__wrapped",
                                "__userDefined",
                                "__userMarshal",
                                DEFAULT_SYMBOL
                            ] {
                                object.shift_remove(key);
                            }

                            let entries = object.iter_mut();
                            self.write_number(entries.len() as i32);

                            for (key, value) in entries {
                                let key_value = if let Some(stripped) = key.strip_prefix("__integer__") {
                                    stripped.parse::<u16>().unwrap().into()
                                } else if let Some(stripped) = key.strip_prefix("__float__") {
                                    stripped.parse::<f64>().unwrap().into()
                                } else if let Some(stripped) = key.strip_prefix("__array__") {
                                    from_str(stripped).unwrap()
                                } else if let Some(stripped) = key.strip_prefix("__object__") {
                                    from_str(stripped).unwrap()
                                } else {
                                    key.as_str().into()
                                };

                                self.write_structure(key_value);
                                self.write_structure(value.take());
                            }

                            if let Some(default_value) = default_value {
                                self.write_structure(default_value);
                            }
                        }
                    }
                    Value::Array(_) => {
                        if !self.objects.contains_key(&value) {
                            self.objects.insert(value.clone(), self.objects.len());
                        }

                        let array = value.as_array_mut().unwrap();
                        self.write_byte(Constants::Array as u8);
                        self.write_number(array.len() as i32);

                        for element in array {
                            self.write_structure(element.take());
                        }
                    }
                    Value::String(_) => {
                        let string = value.as_str().unwrap();

                        if string.starts_with("__symbol__") {
                            self.write_symbol(string.into());
                        } else {
                            if !self.objects.contains_key(&value) {
                                self.objects.insert(value.clone(), self.objects.len());
                            }

                            self.write_byte(Constants::InstanceVar as u8);
                            self.write_byte(Constants::String as u8);
                            self.write_string(string);
                            self.write_number(1);
                            self.write_symbol(ENCODING_SHORT_SYMBOL.into());
                            self.write_byte(Constants::True as u8);
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
///
/// instance_var_prefix argument takes a string, and replaces instance variables' prefixes with Ruby's "@" prefix. It's value must be the same, as in load() function.
/// # Example
/// ```rust
/// use marshal_rs::dump::dump;
/// use serde_json::{json};
///
/// // Value of null
/// let json = json!(null); // null
///
/// // Serialize Value to bytes
/// let bytes: Vec<u8> = dump(json, None);
/// assert_eq!(&bytes, &[0x04, 0x08, 0x30]);
/// ```
pub fn dump(value: Value, instance_var_prefix: Option<&str>) -> Vec<u8> {
    Dumper::new().dump(value, instance_var_prefix)
}
