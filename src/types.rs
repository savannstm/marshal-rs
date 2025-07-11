use bitflags::bitflags;
use gxhash::GxBuildHasher;
use indexmap::{IndexMap, map::Entry};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::{Number, from_str, to_string};
use std::{
    cell::UnsafeCell,
    hash::{Hash, Hasher},
    ops::{Deref, DerefMut, Index, IndexMut},
};
use strum_macros::EnumIs;

use crate::VALUE_INSTANCE_COUNTER;

#[derive(Debug)]
pub(crate) struct SafeCell<T> {
    inner: UnsafeCell<T>,
}

impl<T> SafeCell<T> {
    #[inline]
    pub const fn new(value: T) -> Self {
        SafeCell {
            inner: UnsafeCell::new(value),
        }
    }

    #[allow(clippy::mut_from_ref)]
    #[inline]
    pub fn get(&self) -> &mut T {
        unsafe { &mut *self.inner.get() }
    }

    #[inline]
    pub fn into_inner(self) -> T {
        self.inner.into_inner()
    }
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Object(pub IndexMap<String, Value, GxBuildHasher>);

impl Object {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_capacity(capacity: usize) -> Self {
        Object(IndexMap::with_capacity_and_hasher(
            capacity,
            GxBuildHasher::default(),
        ))
    }
}

impl Deref for Object {
    type Target = IndexMap<String, Value, GxBuildHasher>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for Object {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl FromIterator<(String, Value)> for Object {
    fn from_iter<T: IntoIterator<Item = (String, Value)>>(iter: T) -> Self {
        Object(IndexMap::from_iter(iter))
    }
}

impl<I: IntoIterator<Item = (String, Value)>> From<I> for Object {
    fn from(value: I) -> Self {
        Object(IndexMap::from_iter(value))
    }
}

impl Hash for Object {
    fn hash<H: Hasher>(&self, state: &mut H) {
        let mut kv = Vec::from_iter(&self.0);
        kv.sort_unstable_by(|a, b| a.0.cmp(b.0));
        kv.hash(state);
    }
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct HashMap(pub IndexMap<Value, Value, GxBuildHasher>);

impl HashMap {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_capacity(capacity: usize) -> Self {
        HashMap(IndexMap::with_capacity_and_hasher(
            capacity,
            GxBuildHasher::default(),
        ))
    }
}

impl FromIterator<(Value, Value)> for HashMap {
    fn from_iter<T: IntoIterator<Item = (Value, Value)>>(iter: T) -> Self {
        HashMap(IndexMap::from_iter(iter))
    }
}

impl<I: IntoIterator<Item = (Value, Value)>> From<I> for HashMap {
    fn from(value: I) -> Self {
        HashMap(IndexMap::from_iter(value))
    }
}

impl Deref for HashMap {
    type Target = IndexMap<Value, Value, GxBuildHasher>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for HashMap {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl Hash for HashMap {
    fn hash<H: Hasher>(&self, state: &mut H) {
        let mut hashes: Vec<u64> = self
            .0
            .iter()
            .map(|(k, v)| {
                k.hash(state);
                v.hash(state);
                state.finish()
            })
            .collect();

        hashes.sort_unstable();

        for h in hashes {
            h.hash(state);
        }
    }
}

/// An enum that holds different [`Value`] types.
#[derive(Debug, Default, Clone, EnumIs, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum ValueType {
    #[default]
    Null = 0,
    Bool(bool) = 1,
    Integer(i32) = 2,
    Float(String) = 3,
    Bigint(String) = 4,
    String(String) = 5,
    Bytes(Vec<u8>) = 6,
    Symbol(String) = 7,
    Regexp(String) = 8,
    Array(Vec<Value>) = 9,
    Object(Object) = 10,
    Class = 11,
    Module = 12,
    HashMap(HashMap) = 13,
    Struct(HashMap) = 14,
}

impl ValueType {
    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Self::Bool(b) => Some(*b),
            _ => None,
        }
    }

    pub fn as_float(&self) -> Option<f64> {
        match self {
            Self::Float(n) => n.parse::<f64>().ok(),
            _ => None,
        }
    }

    pub fn as_int(&self) -> Option<i32> {
        match self {
            Self::Integer(n) => Some(*n),
            _ => None,
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Self::String(s)
            | Self::Symbol(s)
            | Self::Regexp(s)
            | Self::Float(s)
            | Self::Bigint(s) => Some(s),
            _ => None,
        }
    }

    pub fn as_array(&self) -> Option<&Vec<Value>> {
        match self {
            Self::Array(a) => Some(a),
            _ => None,
        }
    }

    pub fn as_array_mut(&mut self) -> Option<&mut Vec<Value>> {
        match self {
            Self::Array(a) => Some(a),
            _ => None,
        }
    }

    pub fn as_object(&self) -> Option<&Object> {
        match self {
            Self::Object(o) => Some(o),
            _ => None,
        }
    }

    pub fn as_object_mut(&mut self) -> Option<&mut Object> {
        match self {
            Self::Object(o) => Some(o),
            _ => None,
        }
    }

    pub fn as_hashmap(&self) -> Option<&HashMap> {
        match self {
            Self::HashMap(m) => Some(m),
            _ => None,
        }
    }

    pub fn as_hashmap_mut(&mut self) -> Option<&mut HashMap> {
        match self {
            Self::HashMap(m) => Some(m),
            _ => None,
        }
    }
}

bitflags! {
    #[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash)]
    struct ValueFlags: u8 {
        const None = 0;
        const OldModule = 1;
        const UserClass = 2;
        const Data = 4;
        const UserDefined = 8;
        const UserMarshal = 16;
    }
}

/// A wrapper around [`serde_json::Value`] that tracks an instance ID, class, extensions and flags of the value.
///
/// Use [`Value::null`], [`Value::bool`] etc. to construct the desired value from data.
///
/// To serialize to editable JSON and back, use [`serde_json::to_string`] and [`serde_json::from_str`] respectively.
///
/// # Example
///
/// ```
/// use marshal_rs::{HashMap, Value};
///
/// let value = Value::hash(HashMap::from([
///     (Value::string("name"), Value::string("marshal-rs")),
///     (Value::string("tags"), Value::array([Value::string("hate"), Value::string("ruby")]))
/// ]));
///
/// println!("{:?}", value);
/// ```
#[derive(Debug, Clone, Default, Eq)]
pub struct Value {
    id: usize,
    value: ValueType,
    class: String,
    extensions: Vec<String>,
    flags: ValueFlags,
}

impl Value {
    /// Creates a new null `Value`.
    ///
    /// Does **not** increment the instance counter.
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns the unique ID associated with this `Value`.
    ///
    /// Each `Value` is assigned a unique ID at creation time.
    pub fn id(&self) -> usize {
        self.id
    }

    /// Returns the Ruby class name associated with this `Value`.
    pub fn class_name(&self) -> &str {
        &self.class
    }

    /// Sets the Ruby class name of this `Value`.
    ///
    /// # Arguments
    ///
    /// * `class` - A string representing the new class name.
    pub fn set_class(&mut self, class: String) {
        self.class = class;
    }

    /// Returns the numeric type identifier for the inner `ValueType`.
    ///
    /// This value is an internal representation of the structure type.
    pub fn value_type(&self) -> u8 {
        unsafe { *(&self.value as *const ValueType).cast::<u8>() }
    }

    /// Returns the list of modules that extend this value.
    pub fn extensions(&self) -> &[String] {
        &self.extensions
    }

    /// Adds a module to the list of extensions for this value.
    ///
    /// # Arguments
    ///
    /// * `extension` - The name of the module extending this value.
    pub fn add_extension(&mut self, extension: String) {
        self.extensions.push(extension);
    }

    /// Returns `true` if this value represents an old-style Ruby module.
    ///
    /// Only applies if the `ValueType` is `Module`.
    pub fn is_old_module(&self) -> bool {
        self.flags.contains(ValueFlags::OldModule)
    }

    /// Enables or disables the `OldModule` flag.
    ///
    /// # Arguments
    ///
    /// * `enabled` - If `true`, sets the flag; otherwise, clears it.
    pub fn set_old_module(&mut self, enabled: bool) {
        if enabled {
            self.flags |= ValueFlags::OldModule;
        } else {
            self.flags &= !ValueFlags::OldModule;
        }
    }

    /// Returns `true` if this value is a subclass of `String`, `Regexp`, `Array`, or `Hash`.
    pub fn is_user_class(&self) -> bool {
        self.flags.contains(ValueFlags::UserClass)
    }

    /// Enables or disables the `UserClass` flag.
    ///
    /// # Arguments
    ///
    /// * `enabled` - If `true`, sets the flag; otherwise, clears it.
    pub fn set_user_class(&mut self, enabled: bool) {
        if enabled {
            self.flags |= ValueFlags::UserClass;
        } else {
            self.flags &= !ValueFlags::UserClass;
        }
    }

    /// Returns `true` if this value is of `Data` type.
    pub fn is_data(&self) -> bool {
        self.flags.contains(ValueFlags::Data)
    }

    /// Enables or disables the `Data` flag.
    ///
    /// # Arguments
    ///
    /// * `enabled` - If `true`, sets the flag; otherwise, clears it.
    pub fn set_data(&mut self, enabled: bool) {
        if enabled {
            self.flags |= ValueFlags::Data;
        } else {
            self.flags &= !ValueFlags::Data;
        }
    }

    /// Returns `true` if this value has user-defined `_dump` and `_load` methods.
    pub fn is_user_defined(&self) -> bool {
        self.flags.contains(ValueFlags::UserDefined)
    }

    /// Enables or disables the `UserDefined` flag.
    ///
    /// # Arguments
    ///
    /// * `enabled` - If `true`, sets the flag; otherwise, clears it.
    pub fn set_user_defined(&mut self, enabled: bool) {
        if enabled {
            self.flags |= ValueFlags::UserDefined;
        } else {
            self.flags &= !ValueFlags::UserDefined;
        }
    }

    /// Returns `true` if this value has user-defined `marshal_dump` and `marshal_load` methods.
    pub fn is_user_marshal(&self) -> bool {
        self.flags.contains(ValueFlags::UserMarshal)
    }

    /// Enables or disables the `UserMarshal` flag.
    ///
    /// # Arguments
    ///
    /// * `enabled` - If `true`, sets the flag; otherwise, clears it.
    pub fn set_user_marshal(&mut self, enabled: bool) {
        if enabled {
            self.flags |= ValueFlags::UserMarshal;
        } else {
            self.flags &= !ValueFlags::UserMarshal;
        }
    }

    /// Converts the value into an `Object` if the type is `ValueType::Object`.
    ///
    /// Returns `None` if the value is not an object.
    pub fn into_object(self) -> Option<Object> {
        match self.value {
            ValueType::Object(map) => Some(map),
            _ => None,
        }
    }

    /// Replaces the internal `ValueType` without incrementing the instance counter.
    ///
    /// # Arguments
    ///
    /// * `value` - The new `ValueType` to set.
    pub fn set_value(&mut self, value: ValueType) {
        self.value = value;
    }

    /// Replaces this value with its default and returns the original value.
    pub fn take(&mut self) -> Self {
        std::mem::take(self)
    }

    /// Converts the value into a `Vec<Value>` if it is an array.
    ///
    /// Returns `None` otherwise.
    pub fn into_array(self) -> Option<Vec<Value>> {
        match self.value {
            ValueType::Array(arr) => Some(arr),
            _ => None,
        }
    }

    /// Returns the underlying byte representation if the value is `Bytes`.
    pub fn as_byte_vec(&self) -> Option<&[u8]> {
        match &self.value {
            ValueType::Bytes(arr) => Some(arr),
            _ => None,
        }
    }

    /// Converts the value into a `HashMap` if it is a `ValueType::HashMap`.
    ///
    /// Returns `None` otherwise.
    pub fn into_map(self) -> Option<HashMap> {
        match self.value {
            ValueType::HashMap(m) => Some(m),
            _ => None,
        }
    }

    /// Creates a new null `Value`.
    ///
    /// Increments the instance counter.
    pub fn null() -> Self {
        Self::from(ValueType::Null)
    }

    /// Creates a new boolean `Value`.
    ///
    /// Increments the instance counter.
    ///
    /// # Arguments
    ///
    /// * `bool` - The boolean to wrap.
    pub fn bool(bool: bool) -> Self {
        Self::from(ValueType::Bool(bool))
    }

    /// Creates a new integer `Value`.
    ///
    /// Increments the instance counter.
    ///
    /// # Arguments
    ///
    /// * `int` - The integer to wrap.
    pub fn int(int: i32) -> Self {
        Self::from(ValueType::Integer(int))
    }

    /// Creates a new Value from a slice.
    ///
    /// Increments the instance counter.
    ///
    /// # Arguments
    ///
    /// * `array` - The slice of `Value`s to wrap.
    pub fn array(array: impl AsRef<[Value]>) -> Self {
        Self::from(ValueType::Array(array.as_ref().to_owned()))
    }

    /// Creates a new string `Value`.
    ///
    /// Increments the instance counter.
    ///
    /// # Arguments
    ///
    /// * `str` - The string to wrap.
    pub fn string(str: impl AsRef<str>) -> Self {
        Self::from(ValueType::String(str.as_ref().to_owned()))
    }

    /// Creates a new float `Value` from a string representation.
    ///
    /// Increments the instance counter.
    ///
    /// # Arguments
    ///
    /// * `str` - The string representation of a float.
    pub fn float(str: impl AsRef<str>) -> Self {
        Self::from(ValueType::Float(str.as_ref().to_owned()))
    }

    /// Creates a new symbol `Value`.
    ///
    /// Increments the instance counter.
    ///
    /// # Arguments
    ///
    /// * `str` - The symbol name to wrap.
    pub fn symbol(str: impl AsRef<str>) -> Self {
        Self::from(ValueType::Symbol(str.as_ref().to_owned()))
    }

    /// Creates a new bigint `Value`.
    ///
    /// Increments the instance counter.
    ///
    /// # Arguments
    ///
    /// * `str` - The string representation of a big integer.
    pub fn bigint(str: impl AsRef<str>) -> Self {
        Self::from(ValueType::Bigint(str.as_ref().to_owned()))
    }

    /// Creates a new `Class` value.
    pub fn class() -> Self {
        Self::from(ValueType::Class)
    }

    /// Creates a new `Module` value.
    pub fn module() -> Self {
        Self::from(ValueType::Module)
    }

    /// Creates a new [`HashMap`] value.
    ///
    /// Increments the instance counter.
    ///
    /// # Arguments
    ///
    /// * `map` - The `HashMap` to wrap.
    pub fn hash(map: HashMap) -> Self {
        Self::from(ValueType::HashMap(map))
    }

    /// Creates a new byte array `Value`.
    ///
    /// Increments the instance counter.
    ///
    /// # Arguments
    ///
    /// * `bytes` - The byte slice to wrap.
    pub fn bytes(bytes: &[u8]) -> Self {
        Self::from(ValueType::Bytes(bytes.to_vec()))
    }

    /// Creates a new regular expression `Value`.
    ///
    /// Increments the instance counter.
    ///
    /// # Arguments
    ///
    /// * `str` - The regex pattern to wrap.
    pub fn regexp(str: impl AsRef<str>) -> Self {
        Self::from(ValueType::Regexp(str.as_ref().to_owned()))
    }

    /// Creates a new object `Value`.
    ///
    /// Increments the instance counter.
    ///
    /// # Arguments
    ///
    /// * `object` - The object map to wrap.
    pub fn object(object: Object) -> Self {
        Self::from(ValueType::Object(object))
    }

    /// Creates a new struct `Value` from a [`HashMap`].
    ///
    /// Increments the instance counter.
    ///
    /// # Arguments
    ///
    /// * `map` - The struct map to wrap.
    pub fn rstruct(map: HashMap) -> Self {
        Self::from(ValueType::Struct(map))
    }

    /// Returns a reference to the value corresponding to the key if the Value is object.
    ///
    /// Returns None if not an object or key doesn't exist.
    pub fn get(&self, key: &str) -> Option<&Value> {
        match &self.value {
            ValueType::Object(obj) => obj.get(key),
            _ => None,
        }
    }
}

impl From<ValueType> for Value {
    /// Converts [`ValueType`] to [`Value`].
    ///
    /// To construct `Value` and set its class, type etc., use [`serde_json::from_str`] with a string that has been serialized to JSON from `Value` using [`serde_json::to_string`], and has `__id`, `__value`, `__class`, `__type`, `__extensions`, and `__flags` fields set.
    ///
    /// Increments the instance counter.
    fn from(value: ValueType) -> Self {
        let result = Self {
            id: VALUE_INSTANCE_COUNTER.with(|x| *x.get()),
            value,
            class: String::new(),
            extensions: Vec::new(),
            flags: ValueFlags::None,
        };

        VALUE_INSTANCE_COUNTER.with(|x| *x.get() += 1);
        result
    }
}

impl PartialEq for Value {
    fn eq(&self, other: &Self) -> bool {
        self.value == other.value
            && self.class == other.class
            && self.extensions == other.extensions
            && self.flags == other.flags
    }
}

impl Hash for Value {
    fn hash<H: Hasher>(&self, state: &mut H) {
        state.write_u8(0);
        self.value.hash(state);

        state.write_u8(1);
        self.class.hash(state);

        state.write_u8(2);
        self.extensions.hash(state);

        state.write_u8(3);
        self.flags.hash(state);
    }
}

impl Deref for Value {
    type Target = ValueType;

    fn deref(&self) -> &Self::Target {
        &self.value
    }
}

impl DerefMut for Value {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.value
    }
}

impl Index<&str> for Value {
    type Output = Value;

    fn index(&self, key: &str) -> &Self::Output {
        match &self.value {
            ValueType::Object(obj) => &obj[key],
            _ => panic!("Cannot index into a Value that is not an object"),
        }
    }
}

impl IndexMut<&str> for Value {
    fn index_mut(&mut self, key: &str) -> &mut Self::Output {
        match &mut self.value {
            ValueType::Object(obj) => match obj.entry(key.to_owned()) {
                Entry::Occupied(entry) => entry.into_mut(),
                Entry::Vacant(entry) => entry.insert(Value::default()),
            },
            _ => panic!("Cannot index into a Value that is not an object"),
        }
    }
}

impl Index<usize> for Value {
    type Output = Value;

    fn index(&self, idx: usize) -> &Self::Output {
        match &self.value {
            ValueType::Array(arr) => &arr[idx],
            _ => panic!("Cannot index into a Value that is not an array"),
        }
    }
}

impl IndexMut<usize> for Value {
    fn index_mut(&mut self, idx: usize) -> &mut Self::Output {
        match &mut self.value {
            ValueType::Array(arr) => &mut arr[idx],
            _ => panic!("Cannot index into a Value that is not an array"),
        }
    }
}

impl From<serde_json::Value> for Value {
    fn from(value: serde_json::Value) -> Self {
        match value {
            serde_json::Value::Null => Value::null(),
            serde_json::Value::Bool(bool) => Value::bool(bool),
            serde_json::Value::String(str) => Value::string(str),
            serde_json::Value::Number(num) => {
                if let Some(int) = num.as_i64() {
                    Value::int(int as i32)
                } else {
                    Value::float(num.as_f64().unwrap().to_string())
                }
            }
            serde_json::Value::Array(arr) => Value::array(
                arr.into_iter().map(Value::from).collect::<Vec<_>>(),
            ),
            serde_json::Value::Object(obj) => Value::object(
                obj.into_iter().map(|(k, v)| (k, Value::from(v))).collect(),
            ),
        }
    }
}

impl From<Value> for serde_json::Value {
    fn from(value: Value) -> Self {
        match value.value {
            ValueType::Null => serde_json::Value::Null,
            ValueType::Bool(b) => serde_json::Value::Bool(b),
            ValueType::String(s) => serde_json::Value::String(s),
            ValueType::Integer(i) => serde_json::Value::Number(i.into()),
            ValueType::Float(f) => match f.parse::<f64>() {
                Ok(f_num) => serde_json::Number::from_f64(f_num)
                    .map(serde_json::Value::Number)
                    .unwrap_or(serde_json::Value::Null),
                Err(_) => serde_json::Value::Null,
            },
            ValueType::Array(arr) => serde_json::Value::Array(
                arr.into_iter().map(serde_json::Value::from).collect(),
            ),
            ValueType::Object(obj) => serde_json::Value::Object(
                obj.0
                    .into_iter()
                    .map(|(k, v)| (k, serde_json::Value::from(v)))
                    .collect(),
            ),
            _ => unreachable!(),
        }
    }
}

impl Serialize for Value {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        fn to_serializable_value(value: &Value) -> serde_json::Value {
            let mut container_object = serde_json::Map::with_capacity(6);

            container_object.insert(
                "__id".to_owned(),
                serde_json::Value::Number(Number::from(value.id as u64)),
            );

            let serialized_value = match &value.value {
                ValueType::Null => serde_json::Value::Null,
                ValueType::Bool(b) => serde_json::Value::Bool(*b),
                ValueType::Integer(n) => serde_json::Value::from(*n),
                ValueType::Float(s)
                | ValueType::Bigint(s)
                | ValueType::String(s)
                | ValueType::Symbol(s)
                | ValueType::Regexp(s) => serde_json::Value::String(s.clone()),
                ValueType::Bytes(arr) => {
                    let vec = arr
                        .iter()
                        .map(|v| serde_json::Value::from(*v))
                        .collect::<Vec<_>>();
                    serde_json::Value::Array(vec)
                }
                ValueType::Array(arr) => {
                    let vec = arr
                        .iter()
                        .map(to_serializable_value)
                        .collect::<Vec<_>>();
                    serde_json::Value::Array(vec)
                }
                ValueType::Object(obj) => {
                    let map = obj
                        .iter()
                        .map(|(k, v)| (k.clone(), to_serializable_value(v)))
                        .collect::<serde_json::Map<String, serde_json::Value>>(
                        );
                    serde_json::Value::Object(map)
                }
                ValueType::Class | ValueType::Module => serde_json::Value::Null,
                ValueType::HashMap(map) | ValueType::Struct(map) => {
                    let mut serialized_map =
                        serde_json::Map::with_capacity(map.len());

                    for (k, v) in map.iter() {
                        let key_value = to_serializable_value(k);
                        let key_string = to_string(&key_value).unwrap();
                        serialized_map
                            .insert(key_string, to_serializable_value(v));
                    }

                    serde_json::Value::Object(serialized_map)
                }
            };

            container_object.insert("__value".to_owned(), serialized_value);
            container_object.insert(
                "__class".to_owned(),
                serde_json::Value::String(value.class.clone()),
            );
            container_object.insert(
                "__type".to_owned(),
                serde_json::Value::Number(Number::from(
                    value.value_type() as u64
                )),
            );
            container_object.insert(
                "__extensions".to_owned(),
                serde_json::Value::Array(
                    value
                        .extensions
                        .iter()
                        .cloned()
                        .map(serde_json::Value::String)
                        .collect(),
                ),
            );
            container_object.insert(
                "__flags".to_owned(),
                serde_json::Value::Number(Number::from(
                    value.flags.bits() as u64
                )),
            );

            serde_json::Value::Object(container_object)
        }

        to_serializable_value(self).serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for Value {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        fn from_serializable_value(
            value: &serde_json::Value,
        ) -> Result<Value, String> {
            let obj =
                value.as_object().ok_or("Expected an object for Value")?;

            let id =
                obj.get("__id")
                    .and_then(serde_json::Value::as_u64)
                    .ok_or("Missing or invalid __id")? as usize;

            let class = obj
                .get("__class")
                .and_then(serde_json::Value::as_str)
                .ok_or("Missing or invalid __class")?
                .to_string();

            let value_type = obj
                .get("__type")
                .and_then(serde_json::Value::as_u64)
                .ok_or("Missing or invalid __type")?;

            let flags = obj
                .get("__flags")
                .and_then(serde_json::Value::as_u64)
                .ok_or("Missing or invalid __flags")?;

            let flags = ValueFlags::from_bits(flags as u8)
                .ok_or("Invalid flags value")?;

            let extensions = obj
                .get("__extensions")
                .and_then(serde_json::Value::as_array)
                .ok_or("Missing or invalid __extensions")?
                .iter()
                .map(|v| {
                    v.as_str()
                        .map(|s| s.to_string())
                        .ok_or("Invalid string in __extensions")
                })
                .collect::<Result<Vec<_>, _>>()?;

            let val = obj.get("__value").ok_or("Missing __value")?;

            let value = match value_type {
                0 => ValueType::Null,
                1 => ValueType::Bool(val.as_bool().unwrap()),
                2 => ValueType::Integer(val.as_i64().unwrap() as i32),
                3 => ValueType::Float(val.as_str().unwrap().to_owned()),
                4 => ValueType::Bigint(val.as_str().unwrap().to_owned()),
                5 => ValueType::String(val.as_str().unwrap().to_owned()),
                6 => {
                    let vec = val
                        .as_array()
                        .unwrap()
                        .iter()
                        .map(|x| x.as_u64().unwrap() as u8)
                        .collect::<Vec<u8>>();
                    ValueType::Bytes(vec)
                }
                7 => ValueType::Symbol(val.as_str().unwrap().to_owned()),
                8 => ValueType::Regexp(val.as_str().unwrap().to_owned()),
                9 => {
                    let vec = val
                        .as_array()
                        .unwrap()
                        .iter()
                        .map(from_serializable_value)
                        .collect::<Result<Vec<_>, _>>()?;
                    ValueType::Array(vec)
                }
                10 => {
                    let parsed = val
                        .as_object()
                        .unwrap()
                        .iter()
                        .map(|(k, v)| {
                            from_serializable_value(v).map(|uv| (k.clone(), uv))
                        })
                        .collect::<Result<_, _>>()?;
                    ValueType::Object(parsed)
                }
                11 => ValueType::Class,
                12 => ValueType::Module,
                13 => {
                    let parsed = val
                        .as_object()
                        .unwrap()
                        .iter()
                        .map(|(k, v)| {
                            from_serializable_value(v).map(|uv| {
                                (
                                    from_serializable_value(
                                        &from_str(k).unwrap(),
                                    )
                                    .unwrap(),
                                    uv,
                                )
                            })
                        })
                        .collect::<Result<_, _>>()?;
                    ValueType::HashMap(parsed)
                }
                14 => {
                    let parsed = val
                        .as_object()
                        .unwrap()
                        .iter()
                        .map(|(k, v)| {
                            from_serializable_value(v).map(|uv| {
                                (
                                    from_serializable_value(
                                        &from_str(k).unwrap(),
                                    )
                                    .unwrap(),
                                    uv,
                                )
                            })
                        })
                        .collect::<Result<_, _>>()?;
                    ValueType::Struct(parsed)
                }
                _ => unreachable!(),
            };

            Ok(Value {
                id,
                class,
                extensions,
                flags,
                value,
            })
        }

        let val = serde_json::Value::deserialize(deserializer)?;
        from_serializable_value(&val).map_err(serde::de::Error::custom)
    }
}
