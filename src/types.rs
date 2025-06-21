use gxhash::GxBuildHasher;
use indexmap::{map::Entry, IndexMap};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::{json, Map, Value};
use std::{
    cell::UnsafeCell,
    ops::{Deref, DerefMut, Index, IndexMut},
    rc::Rc,
};
use uuid::Uuid;

pub(crate) struct SafeCell<T> {
    inner: UnsafeCell<T>,
}

impl<T> SafeCell<T> {
    #[inline]
    pub fn new(value: T) -> Self {
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

#[derive(Debug, Clone, Default)]
/// A wrapper around `serde_json::Value` that assigns a UUID to every value and tracks object and array children.
///
/// `UuidValue` dereferences to the inner `Value` and can be traversed like a tree structure.
///
/// It also implements `PartialEq` for `Value`, so you can compare directly.
///
/// Use [`UuidValue::from`] or [`uuid_json!`] to construct from a `serde_json::Value`, and [`UuidValue::into_value`] to convert it back.
///
/// # Examples
///
/// ```rust
/// use marshal_rs::{uuid_json, types::UuidValue};
///
/// let uuid_value = uuid_json!({
///     "name": "marshal-rs",
///     "tags": ["hate", "ruby"]
/// });
///
/// if let Some(obj) = uuid_value.as_object() {
///     for (key, child) in obj {
///         println!("Key: {}, UUID: {}", key, child.uuid());
///     }
/// }
/// ```
pub struct UuidValue {
    uuid: Uuid,
    value: Value,
    object_entries: IndexMap<String, UuidValue, GxBuildHasher>,
    array_entries: Vec<UuidValue>,
}

impl UuidValue {
    /// Creates a new empty `UuidValue` using the `Default` implementation.
    pub fn new() -> Self {
        Self::default()
    }

    /// Constructs a `UuidValue` from a given `serde_json::Value`, recursively assigning UUIDs to all nested values.
    pub fn from(value: Value) -> Self {
        let uuid = Uuid::new_v4();

        match value {
            Value::Object(map) => {
                let mut children =
                    IndexMap::with_capacity_and_hasher(map.len(), GxBuildHasher::default());

                for (key, value) in map {
                    children.insert(key, UuidValue::from(value));
                }

                UuidValue {
                    uuid,
                    value: Value::Object(Map::new()),
                    object_entries: children,
                    array_entries: vec![],
                }
            }
            Value::Array(arr) => {
                let array_children = arr.into_iter().map(Self::from).collect();

                UuidValue {
                    uuid,
                    value: Value::Array(vec![]),
                    object_entries: IndexMap::default(),
                    array_entries: array_children,
                }
            }
            primitive => UuidValue {
                uuid,
                value: primitive,
                object_entries: IndexMap::default(),
                array_entries: vec![],
            },
        }
    }

    /// Returns the UUID associated with this value.
    ///
    /// Each `UuidValue` is guaranteed to have a unique UUID upon creation.
    pub fn uuid(&self) -> Uuid {
        self.uuid
    }

    /// Returns a reference to the inner `serde_json::Value`.
    ///
    /// Will return empty object, if value is `Object` or `Array`.
    ///
    /// To get the actual object or array, use `as_object` or `as_array`.
    pub fn value(&self) -> &Value {
        &self.value
    }

    /// Converts the `UuidValue` back into a `serde_json::Value`, discarding UUID metadata.
    pub fn into_value(self) -> Value {
        match self.value {
            Value::Object(_) => {
                let map: Map<String, Value> = self
                    .object_entries
                    .into_iter()
                    .map(|(k, v)| (k, v.into_value()))
                    .collect();
                Value::Object(map)
            }
            Value::Array(_) => {
                let arr: Vec<Value> = self
                    .array_entries
                    .into_iter()
                    .map(|v| v.into_value())
                    .collect();
                Value::Array(arr)
            }
            primitive => primitive,
        }
    }

    /// Replaces this `UuidValue` with the default and returns the original.
    pub fn take(&mut self) -> UuidValue {
        std::mem::take(self)
    }

    /// If this value is an object, returns a reference to its entries.
    pub fn as_object(&self) -> Option<&IndexMap<String, UuidValue, GxBuildHasher>> {
        match self.value {
            Value::Object(_) => Some(&self.object_entries),
            _ => None,
        }
    }

    /// If this value is an array, returns a reference to its elements.
    pub fn as_array(&self) -> Option<&Vec<UuidValue>> {
        match self.value {
            Value::Array(_) => Some(&self.array_entries),
            _ => None,
        }
    }

    /// If this value is an object, returns a mutable reference to its entries.
    pub fn as_object_mut(&mut self) -> Option<&mut IndexMap<String, UuidValue, GxBuildHasher>> {
        match self.value {
            Value::Object(_) => Some(&mut self.object_entries),
            _ => None,
        }
    }

    /// If this value is an array, returns a mutable reference to its elements.
    pub fn as_array_mut(&mut self) -> Option<&mut Vec<UuidValue>> {
        match self.value {
            Value::Array(_) => Some(&mut self.array_entries),
            _ => None,
        }
    }

    /// Retrieves a reference to the value associated with the given key, if this is an object.
    pub fn get(&self, key: &str) -> Option<&UuidValue> {
        match self.value {
            Value::Object(_) => self.object_entries.get(key),
            _ => None,
        }
    }

    /// Retrieves a mutable reference to the value associated with the given key, if this is an object.
    pub fn get_mut(&mut self, key: &str) -> Option<&mut UuidValue> {
        match self.value {
            Value::Object(_) => self.object_entries.get_mut(key),
            _ => None,
        }
    }

    /// Retrieves a reference to the element at the given index, if this is an array.
    pub fn get_index(&self, index: usize) -> Option<&UuidValue> {
        match self.value {
            Value::Array(_) => self.array_entries.get(index),
            _ => None,
        }
    }

    /// Retrieves a mutable reference to the element at the given index, if this is an array.
    pub fn get_index_mut(&mut self, index: usize) -> Option<&mut UuidValue> {
        match self.value {
            Value::Array(_) => self.array_entries.get_mut(index),
            _ => None,
        }
    }
}

impl PartialEq<Value> for UuidValue {
    fn eq(&self, other: &Value) -> bool {
        if std::mem::discriminant(&self.value) != std::mem::discriminant(other) {
            return false;
        }

        println!("{:?}", self.clone().into_value());

        match &self.value {
            Value::Object(_) => self.clone().into_value() == *other,
            Value::Array(_) => self.clone().into_value() == *other,
            primitive => primitive == other,
        }
    }
}

impl PartialEq for UuidValue {
    fn eq(&self, other: &Self) -> bool {
        if std::mem::discriminant(&self.value) != std::mem::discriminant(&other.value) {
            return false;
        }

        match &self.value {
            Value::Object(_) => self.clone().into_value() == other.clone().into_value(),
            Value::Array(_) => self.clone().into_value() == other.clone().into_value(),
            primitive => primitive == &other.value,
        }
    }
}

impl Deref for UuidValue {
    type Target = Value;

    fn deref(&self) -> &Self::Target {
        &self.value
    }
}

impl DerefMut for UuidValue {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.value
    }
}

impl Index<&str> for UuidValue {
    type Output = UuidValue;

    fn index(&self, key: &str) -> &Self::Output {
        &self.object_entries[key]
    }
}

impl IndexMut<&str> for UuidValue {
    fn index_mut(&mut self, key: &str) -> &mut Self::Output {
        match self.object_entries.entry(key.to_owned()) {
            Entry::Occupied(entry) => entry.into_mut(),
            Entry::Vacant(entry) => entry.insert(UuidValue::default()),
        }
    }
}

impl Index<usize> for UuidValue {
    type Output = UuidValue;

    fn index(&self, idx: usize) -> &Self::Output {
        &self.array_entries[idx]
    }
}

impl IndexMut<usize> for UuidValue {
    fn index_mut(&mut self, idx: usize) -> &mut Self::Output {
        &mut self.array_entries[idx]
    }
}

// TODO: make this serialization shit work

pub fn wrap_with_uuid(value: &UuidValue) -> Value {
    match &value.value {
        Value::Object(_) => {
            let wrapped_map = value
                .object_entries
                .iter()
                .map(|(k, v)| (k.clone(), wrap_with_uuid(v)))
                .collect::<Map<_, _>>();

            json!({ "uuid": value.uuid(), "value": wrapped_map })
        }

        Value::Array(_) => {
            let wrapped_array = value
                .array_entries
                .iter()
                .map(wrap_with_uuid)
                .collect::<Vec<_>>();
            json!({ "uuid": value.uuid(), "value": wrapped_array })
        }

        primitive => {
            json!({
                "uuid": value.uuid(),
                "value": primitive
            })
        }
    }
}

impl Serialize for UuidValue {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        self.clone().into_value().serialize(serializer)
    }
}

fn unwrap_with_uuid(wrapped: &Value) -> Result<UuidValue, String> {
    let uuid_str = wrapped
        .get("uuid")
        .and_then(Value::as_str)
        .ok_or("Missing or invalid 'uuid' field")?;
    let uuid = Uuid::parse_str(uuid_str).map_err(|e| format!("Invalid UUID: {}", e))?;

    let inner_value = wrapped.get("value").ok_or("Missing 'value' field")?;

    match inner_value {
        Value::Object(obj) => {
            let mut object_entries =
                IndexMap::with_capacity_and_hasher(obj.len(), GxBuildHasher::default());

            for (k, v) in obj {
                let child = unwrap_with_uuid(v)?;
                object_entries.insert(k.clone(), child);
            }

            Ok(UuidValue {
                uuid,
                value: Value::Object(
                    object_entries
                        .iter()
                        .map(|(k, v)| (k.clone(), v.value.clone()))
                        .collect(),
                ),
                object_entries,
                array_entries: vec![],
            })
        }

        Value::Array(arr) => {
            let mut array_entries = Vec::new();
            for v in arr {
                array_entries.push(unwrap_with_uuid(v)?);
            }

            Ok(UuidValue {
                uuid,
                value: Value::Array(array_entries.iter().map(|v| v.value.clone()).collect()),
                object_entries: IndexMap::default(),
                array_entries,
            })
        }

        primitive => Ok(UuidValue {
            uuid,
            value: primitive.clone(),
            object_entries: IndexMap::default(),
            array_entries: vec![],
        }),
    }
}

impl<'de> Deserialize<'de> for UuidValue {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = Value::deserialize(deserializer)?;
        Ok(UuidValue::from(value))
    }
}

pub(crate) type ValueRc = Rc<SafeCell<UuidValue>>;

#[macro_export]
macro_rules! uuid_json {
    ($($json:tt)+) => {
        $crate::types::UuidValue::from(serde_json::json!($($json)+))
    };
}

#[macro_export]
macro_rules! value_rc {
    ($val:expr) => {
        std::rc::Rc::new(SafeCell::new($val))
    };
}
