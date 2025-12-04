use crate::VALUE_INSTANCE_COUNTER;
use bitflags::bitflags;
use gxhash::GxBuildHasher;
use indexmap::{IndexMap, map::Entry};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::{Number, from_str, to_string, to_string_pretty};
use std::{
    cell::UnsafeCell,
    fmt::Display,
    hash::{Hash, Hasher},
    ops::{Deref, DerefMut, Index, IndexMut},
    str::FromStr,
};
use strum_macros::EnumIs;

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
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    #[must_use]
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
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    #[must_use]
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
    #[must_use]
    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Self::Bool(b) => Some(*b),
            _ => None,
        }
    }

    #[must_use]
    pub fn as_float(&self) -> Option<f64> {
        match self {
            Self::Float(n) => n.parse::<f64>().ok(),
            _ => None,
        }
    }

    #[must_use]
    pub fn as_int(&self) -> Option<i32> {
        match self {
            Self::Integer(n) => Some(*n),
            _ => None,
        }
    }

    #[must_use]
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

    #[must_use]
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

    #[must_use]
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

    #[must_use]
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
        const OldModule = 1 << 0;
        const UserClass = 1 << 1;
        const Data = 1 << 2;
        const UserDefined = 1 << 3;
        const UserMarshal = 1 << 4;
    }
}

/// A wrapper around [`serde_json::Value`] that tracks an instance ID, class, extensions and flags of the value.
///
/// Use [`Value::null`], [`Value::bool`] etc. to construct the desired value from data.
///
/// To serialize to editable JSON and back, use [`Value::to_string`] and [`Value::from_str`] respectively.
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
    val: ValueType,
    class: String,
    extensions: Vec<String>,
    flags: ValueFlags,
}

impl Value {
    /// Creates a new null [`Value`].
    ///
    /// Does **not** increment the instance counter.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns the unique ID associated with this [`Value`].
    ///
    /// Each [`Value`] is assigned a unique ID at creation time.
    #[must_use]
    pub fn id(&self) -> usize {
        self.id
    }

    /// Returns the Ruby class name associated with this [`Value`].
    #[must_use]
    pub fn class_name(&self) -> &str {
        &self.class
    }

    /// Sets the Ruby class name of this [`Value`].
    ///
    /// # Arguments
    ///
    /// - `class` - A string representing the new class name.
    pub fn set_class(&mut self, class: String) {
        self.class = class;
    }

    /// Returns the numeric type identifier for the inner [`ValueType`].
    ///
    /// This value is an internal representation of the structure type.
    #[must_use]
    pub fn value_type(&self) -> u8 {
        unsafe { *(&raw const self.val).cast::<u8>() }
    }

    /// Returns the list of modules that extend this value.
    #[must_use]
    pub fn extensions(&self) -> &[String] {
        &self.extensions
    }

    /// Adds a module to the list of extensions for this value.
    ///
    /// # Arguments
    ///
    /// - `extension` - The name of the module extending this value.
    pub fn add_extension(&mut self, extension: String) {
        self.extensions.push(extension);
    }

    /// Returns [`true`] if this value represents an old-style Ruby module.
    ///
    /// Only applies if the [`ValueType`] is [`ValueType::Module`].
    #[must_use]
    pub fn is_old_module(&self) -> bool {
        self.flags.contains(ValueFlags::OldModule)
    }

    /// Enables or disables the [`ValueFlags::OldModule`] flag.
    ///
    /// # Arguments
    ///
    /// - `enabled` - If `true`, sets the flag; otherwise, clears it.
    pub fn set_old_module(&mut self, enabled: bool) {
        if enabled {
            self.flags |= ValueFlags::OldModule;
        } else {
            self.flags &= !ValueFlags::OldModule;
        }
    }

    /// Returns `true` if this value is a subclass of `String`, `Regexp`, `Array`, or `Hash`.
    #[must_use]
    pub fn is_user_class(&self) -> bool {
        self.flags.contains(ValueFlags::UserClass)
    }

    /// Enables or disables the `UserClass` flag.
    ///
    /// # Arguments
    ///
    /// - `enabled` - If [`true`], sets the flag; otherwise, clears it.
    pub fn set_user_class(&mut self, enabled: bool) {
        if enabled {
            self.flags |= ValueFlags::UserClass;
        } else {
            self.flags &= !ValueFlags::UserClass;
        }
    }

    /// Returns [`true`] if this value is of [`ValueFlags::Data`] type.
    #[must_use]
    pub fn is_data(&self) -> bool {
        self.flags.contains(ValueFlags::Data)
    }

    /// Enables or disables the [`ValueFlags::Data`] flag.
    ///
    /// # Arguments
    ///
    /// - `enabled` - If [`true`], sets the flag; otherwise, clears it.
    pub fn set_data(&mut self, enabled: bool) {
        if enabled {
            self.flags |= ValueFlags::Data;
        } else {
            self.flags &= !ValueFlags::Data;
        }
    }

    /// Returns [`true`] if this value has user-defined `_dump` and `_load` methods.
    #[must_use]
    pub fn is_user_defined(&self) -> bool {
        self.flags.contains(ValueFlags::UserDefined)
    }

    /// Enables or disables the [`ValueFlags::UserDefined`] flag.
    ///
    /// # Arguments
    ///
    /// - `enabled` - If [`true`], sets the flag; otherwise, clears it.
    pub fn set_user_defined(&mut self, enabled: bool) {
        if enabled {
            self.flags |= ValueFlags::UserDefined;
        } else {
            self.flags &= !ValueFlags::UserDefined;
        }
    }

    /// Returns `true` if this value has user-defined `marshal_dump` and `marshal_load` methods.
    #[must_use]
    pub fn is_user_marshal(&self) -> bool {
        self.flags.contains(ValueFlags::UserMarshal)
    }

    /// Enables or disables the `UserMarshal` flag.
    ///
    /// # Arguments
    ///
    /// - `enabled` - If [`true`], sets the flag; otherwise, clears it.
    pub fn set_user_marshal(&mut self, enabled: bool) {
        if enabled {
            self.flags |= ValueFlags::UserMarshal;
        } else {
            self.flags &= !ValueFlags::UserMarshal;
        }
    }

    /// Converts the value into an [`Object`] if the type is [`ValueType::Object`].
    ///
    /// Returns [`None`] if the value is not an object.
    #[must_use]
    pub fn into_object(self) -> Option<Object> {
        match self.val {
            ValueType::Object(map) => Some(map),
            _ => None,
        }
    }

    /// Replaces the internal [`ValueType`] without incrementing the instance counter.
    ///
    /// # Arguments
    ///
    /// - `value` - The new [`ValueType`] to set.
    pub fn set_value(&mut self, value: ValueType) {
        self.val = value;
    }

    /// Replaces this value with its default and returns the original value.
    #[must_use]
    pub fn take(&mut self) -> Self {
        std::mem::take(self)
    }

    /// Converts the value into a `Vec<Value>` if it is an array.
    ///
    /// Returns `None` otherwise.
    #[must_use]
    pub fn into_array(self) -> Option<Vec<Value>> {
        match self.val {
            ValueType::Array(arr) => Some(arr),
            _ => None,
        }
    }

    /// Returns the underlying byte representation if the value is `Bytes`.
    #[must_use]
    pub fn as_byte_vec(&self) -> Option<&[u8]> {
        match &self.val {
            ValueType::Bytes(arr) => Some(arr),
            _ => None,
        }
    }

    /// Converts the value into a [`HashMap`] if it is a [`ValueType::HashMap`].
    ///
    /// Returns [`None`] otherwise.
    #[must_use]
    pub fn into_map(self) -> Option<HashMap> {
        match self.val {
            ValueType::HashMap(m) => Some(m),
            _ => None,
        }
    }

    /// Creates a new null [`Value`].
    ///
    /// Increments the instance counter.
    #[must_use]
    pub fn null() -> Self {
        Self::from(ValueType::Null)
    }

    /// Creates a new boolean [`Value`].
    ///
    /// Increments the instance counter.
    ///
    /// # Arguments
    ///
    /// - `bool` - The boolean to wrap.
    #[must_use]
    pub fn bool(bool: bool) -> Self {
        Self::from(ValueType::Bool(bool))
    }

    /// Creates a new integer [`Value`].
    ///
    /// Increments the instance counter.
    ///
    /// # Arguments
    ///
    /// - `int` - The integer to wrap.
    #[must_use]
    pub fn int(int: i32) -> Self {
        Self::from(ValueType::Integer(int))
    }

    /// Creates a new Value from a slice.
    ///
    /// Increments the instance counter.
    ///
    /// # Arguments
    ///
    /// - `array` - The slice of [`Value`]s to wrap.
    pub fn array(array: impl AsRef<[Value]>) -> Self {
        Self::from(ValueType::Array(array.as_ref().to_owned()))
    }

    /// Creates a new string [`Value`].
    ///
    /// Increments the instance counter.
    ///
    /// # Arguments
    ///
    /// - `str` - The string to wrap.
    pub fn string(str: impl AsRef<str>) -> Self {
        Self::from(ValueType::String(str.as_ref().to_owned()))
    }

    /// Creates a new float [`Value`] from a string representation.
    ///
    /// Increments the instance counter.
    ///
    /// # Arguments
    ///
    /// - `str` - The string representation of a float.
    pub fn float(str: impl AsRef<str>) -> Self {
        Self::from(ValueType::Float(str.as_ref().to_owned()))
    }

    /// Creates a new symbol [`Value`].
    ///
    /// Increments the instance counter.
    ///
    /// # Arguments
    ///
    /// - `str` - The symbol name to wrap.
    pub fn symbol(str: impl AsRef<str>) -> Self {
        Self::from(ValueType::Symbol(str.as_ref().to_owned()))
    }

    /// Creates a new bigint [`Value`].
    ///
    /// Increments the instance counter.
    ///
    /// # Arguments
    ///
    /// - `str` - The string representation of a big integer.
    pub fn bigint(str: impl AsRef<str>) -> Self {
        Self::from(ValueType::Bigint(str.as_ref().to_owned()))
    }

    /// Creates a new [`ValueType::Class`] value.
    #[must_use]
    pub fn class() -> Self {
        Self::from(ValueType::Class)
    }

    /// Creates a new [`ValueType::Module`] value.
    #[must_use]
    pub fn module() -> Self {
        Self::from(ValueType::Module)
    }

    /// Creates a new [`HashMap`] value.
    ///
    /// Increments the instance counter.
    ///
    /// # Arguments
    ///
    /// - `map` - The [`HashMap`] to wrap.
    #[must_use]
    pub fn hash(map: HashMap) -> Self {
        Self::from(ValueType::HashMap(map))
    }

    /// Creates a new byte array [`Value`].
    ///
    /// Increments the instance counter.
    ///
    /// # Arguments
    ///
    /// - `bytes` - The byte slice to wrap.
    #[must_use]
    pub fn bytes(bytes: &[u8]) -> Self {
        Self::from(ValueType::Bytes(bytes.to_vec()))
    }

    /// Creates a new regular expression [`Value`].
    ///
    /// Increments the instance counter.
    ///
    /// # Arguments
    ///
    /// - `str` - The regex pattern to wrap.
    pub fn regexp(str: impl AsRef<str>) -> Self {
        Self::from(ValueType::Regexp(str.as_ref().to_owned()))
    }

    /// Creates a new object [`Value`].
    ///
    /// Increments the instance counter.
    ///
    /// # Arguments
    ///
    /// - `object` - The object map to wrap.
    #[must_use]
    pub fn object(object: Object) -> Self {
        Self::from(ValueType::Object(object))
    }

    /// Creates a new struct [`Value`] from a [`HashMap`].
    ///
    /// Increments the instance counter.
    ///
    /// # Arguments
    ///
    /// - `map` - The struct map to wrap.
    #[must_use]
    pub fn rstruct(map: HashMap) -> Self {
        Self::from(ValueType::Struct(map))
    }

    /// Returns a reference to the value corresponding to the index if the [`Value`] is array.
    ///
    /// Returns [`None`] if not an array or item at index doesn't exist.
    #[must_use]
    pub fn get_index(&self, idx: usize) -> Option<&Value> {
        match &self.val {
            ValueType::Array(arr) => arr.get(idx),
            _ => None,
        }
    }

    /// Returns a mutable reference to the value corresponding to the index if the [`Value`] is array.
    ///
    /// Returns [`None`] if not an array or item at index doesn't exist.
    pub fn get_index_mut(&mut self, idx: usize) -> Option<&mut Value> {
        match &mut self.val {
            ValueType::Array(arr) => arr.get_mut(idx),
            _ => None,
        }
    }

    /// Similar to `Value::to_string`, but serializes [`Value`] as formatted JSON.
    ///
    /// # Errors
    ///
    /// - [`serde_json::Error`].
    pub fn to_string_pretty(&self) -> Result<String, serde_json::Error> {
        to_string_pretty(self)
    }
}

/// Provides `get` and `get_mut` implementations for [`Value`].
///
/// A separate trait is required because [`Value`] has two distinct,
/// but similar object types: [`ValueType::Object`] and [`ValueType::HashMap`].
///
/// [`Object`] is indexed by [`&str`], whereas [`HashMap`] by [`&Value`].
pub trait Get<T> {
    type Output;
    fn get(&self, key: T) -> Option<&Self::Output>;
    fn get_mut(&mut self, key: T) -> Option<&mut Self::Output>;
}

impl Get<&str> for Value {
    type Output = Value;

    /// Returns a reference to the value corresponding to the key if the [`Value`] is [`ValueType::Object`].
    ///
    /// Returns [`None`] if not an object or key doesn't exist.
    fn get(&self, key: &str) -> Option<&Self::Output> {
        match &self.val {
            ValueType::Object(obj) => obj.get(key),
            _ => None,
        }
    }

    /// Returns a mutable reference to the value corresponding to the key if the [`Value`] is [`ValueType::Object`].
    ///
    /// Returns [`None`] if not an object or key doesn't exist.
    fn get_mut(&mut self, key: &str) -> Option<&mut Self::Output> {
        match &mut self.val {
            ValueType::Object(obj) => obj.get_mut(key),
            _ => None,
        }
    }
}

impl Get<&Value> for Value {
    type Output = Value;

    /// Returns a reference to the value corresponding to the key if the [`Value`] is [`ValueType::HashMap`].
    ///
    /// Returns [`None`] if not a hashmap or key doesn't exist.
    fn get(&self, key: &Value) -> Option<&Self::Output> {
        match &self.val {
            ValueType::HashMap(map) => map.get(key),
            _ => None,
        }
    }

    /// Returns a mutable reference to the value corresponding to the key if the [`Value`] is [`ValueType::HashMap`].
    ///
    /// Returns [`None`] if not a hashmap or key doesn't exist.
    fn get_mut(&mut self, key: &Value) -> Option<&mut Self::Output> {
        match &mut self.val {
            ValueType::HashMap(map) => map.get_mut(key),
            _ => None,
        }
    }
}

impl From<ValueType> for Value {
    /// Converts [`ValueType`] to [`Value`].
    ///
    /// To construct [`Value`] and set its class, type etc., use [`Value::from_str`] with a string that has been serialized to JSON from [`Value`] using [`Value::to_string`], and has `__id`, `__value`, `__class`, `__type`, `__extensions`, and `__flags` fields set.
    ///
    /// Increments the instance counter.
    fn from(value: ValueType) -> Self {
        let result = Self {
            id: VALUE_INSTANCE_COUNTER.with(|x| *x.get()),
            val: value,
            class: String::new(),
            extensions: Vec::new(),
            flags: ValueFlags::empty(),
        };

        VALUE_INSTANCE_COUNTER.with(|x| *x.get() += 1);
        result
    }
}

impl PartialEq for Value {
    fn eq(&self, other: &Self) -> bool {
        self.val == other.val
            && self.class == other.class
            && self.extensions == other.extensions
            && self.flags == other.flags
    }
}

impl Hash for Value {
    fn hash<H: Hasher>(&self, state: &mut H) {
        state.write_u8(0);
        self.val.hash(state);

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
        &self.val
    }
}

impl DerefMut for Value {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.val
    }
}

impl Index<&str> for Value {
    type Output = Value;

    fn index(&self, key: &str) -> &Self::Output {
        match &self.val {
            ValueType::Object(obj) => &obj[key],
            _ => panic!("Cannot index into a Value that is not an object"),
        }
    }
}

impl IndexMut<&str> for Value {
    fn index_mut(&mut self, key: &str) -> &mut Self::Output {
        match &mut self.val {
            ValueType::Object(obj) => match obj.entry(key.to_owned()) {
                Entry::Occupied(entry) => entry.into_mut(),
                Entry::Vacant(entry) => entry.insert(Value::default()),
            },
            _ => panic!("Cannot index into a Value that is not an object"),
        }
    }
}

impl Index<Value> for Value {
    type Output = Value;

    fn index(&self, index: Value) -> &Self::Output {
        match &self.val {
            ValueType::HashMap(map) => &map[&index],
            _ => panic!("Cannot index into a Value that is not a hashmap"),
        }
    }
}

impl IndexMut<Value> for Value {
    fn index_mut(&mut self, index: Value) -> &mut Self::Output {
        match &mut self.val {
            ValueType::HashMap(map) => &mut map[&index],
            _ => panic!("Cannot index into a Value that is not a hashmap"),
        }
    }
}

impl Index<usize> for Value {
    type Output = Value;

    fn index(&self, idx: usize) -> &Self::Output {
        match &self.val {
            ValueType::Array(arr) => &arr[idx],
            _ => panic!("Cannot index into a Value that is not an array"),
        }
    }
}

impl IndexMut<usize> for Value {
    fn index_mut(&mut self, idx: usize) -> &mut Self::Output {
        match &mut self.val {
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
        match value.val {
            ValueType::Null => serde_json::Value::Null,
            ValueType::Bool(b) => serde_json::Value::Bool(b),
            ValueType::String(s) => serde_json::Value::String(s),
            ValueType::Integer(i) => serde_json::Value::Number(i.into()),
            ValueType::Float(f) => match f.parse::<f64>() {
                Ok(f_num) => serde_json::Number::from_f64(f_num)
                    .map_or(serde_json::Value::Null, serde_json::Value::Number),
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

impl Display for Value {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&to_string(self).unwrap())
    }
}

impl FromStr for Value {
    type Err = serde_json::Error;

    /// Deserializes value string to an actual [`Value`].
    ///
    /// Wrapper around `serde_json::from_str`
    ///
    /// # Errors
    ///
    /// - [`serde_json::Error`].
    fn from_str(str: &str) -> Result<Self, Self::Err> {
        from_str::<Self>(str)
    }
}

impl Serialize for Value {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        fn serialize_plain<S>(v: &Value, s: S) -> Result<S::Ok, S::Error>
        where
            S: Serializer,
        {
            match &v.val {
                ValueType::Null => s.serialize_unit(),
                ValueType::Bool(b) => s.serialize_bool(*b),
                ValueType::Integer(n) => s.serialize_i32(*n),
                _ => unreachable!(),
            }
        }

        fn to_serializable_value(value: &Value) -> serde_json::Value {
            match &value.val {
                ValueType::Null => return serde_json::Value::Null,
                ValueType::Bool(b) => return serde_json::Value::Bool(*b),
                ValueType::Integer(n) => return serde_json::Value::from(*n),
                _ => {}
            }

            let mut container_object = serde_json::Map::with_capacity(6);

            container_object.insert(
                "__id".to_owned(),
                serde_json::Value::Number(Number::from(value.id as u64)),
            );

            let serialized_value = match &value.val {
                ValueType::Null => serde_json::Value::Null,
                ValueType::Bool(b) => serde_json::Value::Bool(*b),
                ValueType::Integer(n) => serde_json::Value::from(*n),

                ValueType::Float(s)
                | ValueType::Bigint(s)
                | ValueType::String(s)
                | ValueType::Symbol(s)
                | ValueType::Regexp(s) => serde_json::Value::String(s.clone()),

                ValueType::Bytes(arr) => serde_json::Value::Array(
                    arr.iter().map(|v| serde_json::Value::from(*v)).collect(),
                ),

                ValueType::Array(arr) => serde_json::Value::Array(
                    arr.iter().map(to_serializable_value).collect(),
                ),

                ValueType::Object(obj) => {
                    let map = obj
                        .iter()
                        .map(|(k, v)| (k.clone(), to_serializable_value(v)))
                        .collect();
                    serde_json::Value::Object(map)
                }

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

                ValueType::Class | ValueType::Module => serde_json::Value::Null,
            };

            container_object.insert("__value".to_owned(), serialized_value);

            if !value.class.is_empty() {
                container_object.insert(
                    "__class".to_owned(),
                    serde_json::Value::String(value.class.clone()),
                );
            }

            if !value.extensions.is_empty() {
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
            }

            if !value.flags.is_empty() {
                container_object.insert(
                    "__flags".to_owned(),
                    serde_json::Value::Number(Number::from(u64::from(
                        value.flags.bits(),
                    ))),
                );
            }

            container_object.insert(
                "__type".to_owned(),
                serde_json::Value::Number(Number::from(u64::from(
                    value.value_type(),
                ))),
            );

            serde_json::Value::Object(container_object)
        }

        match self.val {
            ValueType::Null | ValueType::Bool(_) | ValueType::Integer(_) => {
                serialize_plain(self, serializer)
            }

            _ => to_serializable_value(self).serialize(serializer),
        }
    }
}

impl<'de> Deserialize<'de> for Value {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        fn decode_plain(v: &serde_json::Value) -> Option<Value> {
            match v {
                serde_json::Value::Null => Some(Value {
                    id: usize::MAX,
                    val: ValueType::Null,
                    class: String::new(),
                    extensions: vec![],
                    flags: ValueFlags::empty(),
                }),

                serde_json::Value::Bool(b) => Some(Value {
                    id: usize::MAX,
                    val: ValueType::Bool(*b),
                    class: String::new(),
                    extensions: vec![],
                    flags: ValueFlags::empty(),
                }),

                serde_json::Value::Number(n) if n.is_i64() => Some(Value {
                    id: usize::MAX,
                    val: ValueType::Integer(n.as_i64().unwrap() as i32),
                    class: String::new(),
                    extensions: vec![],
                    flags: ValueFlags::empty(),
                }),

                _ => None,
            }
        }

        fn from_serializable_value(
            value: &serde_json::Value,
        ) -> Result<Value, String> {
            if !value.is_object() {
                if let Some(v) = decode_plain(value) {
                    return Ok(v);
                }
                return Err("Invalid plain value type".into());
            }

            let obj =
                value.as_object().ok_or("Expected an object for Value")?;

            let id =
                obj.get("__id")
                    .and_then(serde_json::Value::as_u64)
                    .ok_or("Missing or invalid __id")? as usize;

            let value_type = obj
                .get("__type")
                .and_then(serde_json::Value::as_u64)
                .ok_or("Missing or invalid __type")?;

            let raw = obj.get("__value").ok_or("Missing __value")?;

            let class = obj
                .get("__class")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_owned();

            let extensions = obj
                .get("__extensions")
                .and_then(|v| v.as_array())
                .map_or(Ok(vec![]), |arr| {
                    arr.iter()
                        .map(|x| {
                            x.as_str()
                                .map(ToString::to_string)
                                .ok_or("Invalid extension")
                        })
                        .collect::<Result<Vec<_>, _>>()
                })?;

            let flags = obj
                .get("__flags")
                .and_then(serde_json::Value::as_u64)
                .map_or(Ok(ValueFlags::empty()), |bits| {
                    ValueFlags::from_bits(bits as u8)
                        .ok_or("Invalid flags value")
                })?;

            let value = match value_type {
                0 => ValueType::Null,
                1 => ValueType::Bool(raw.as_bool().unwrap()),
                2 => ValueType::Integer(raw.as_i64().unwrap() as i32),
                3 => ValueType::Float(raw.as_str().unwrap().to_owned()),
                4 => ValueType::Bigint(raw.as_str().unwrap().to_owned()),
                5 => ValueType::String(raw.as_str().unwrap().to_owned()),
                6 => {
                    let vec = raw
                        .as_array()
                        .unwrap()
                        .iter()
                        .map(|x| x.as_u64().unwrap() as u8)
                        .collect();
                    ValueType::Bytes(vec)
                }
                7 => ValueType::Symbol(raw.as_str().unwrap().to_owned()),
                8 => ValueType::Regexp(raw.as_str().unwrap().to_owned()),
                9 => {
                    let vec = raw
                        .as_array()
                        .unwrap()
                        .iter()
                        .map(from_serializable_value)
                        .collect::<Result<Vec<_>, _>>()?;
                    ValueType::Array(vec)
                }
                10 => {
                    let parsed = raw
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
                13 | 14 => {
                    let parsed = raw
                        .as_object()
                        .unwrap()
                        .iter()
                        .map(|(k, v)| {
                            from_serializable_value(v).map(|uv| {
                                let key_val = from_serializable_value(
                                    &from_str(k).unwrap(),
                                )
                                .unwrap();
                                (key_val, uv)
                            })
                        })
                        .collect::<Result<_, _>>()?;
                    if value_type == 13 {
                        ValueType::HashMap(parsed)
                    } else {
                        ValueType::Struct(parsed)
                    }
                }
                _ => unreachable!(),
            };

            Ok(Value {
                id,
                val: value,
                class,
                extensions,
                flags,
            })
        }

        let val = serde_json::Value::deserialize(deserializer)?;
        from_serializable_value(&val).map_err(serde::de::Error::custom)
    }
}
