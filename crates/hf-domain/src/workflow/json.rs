//! A JSON value model that keeps object key order and integers of any size, with the
//! semantics Python's `dict` and `int` give a decoded definition.
pub use super::json_int::JsonInt;

/// A decoded JSON value. Integers and floats are distinct, as in Python.
#[derive(Clone, Debug, PartialEq)]
pub enum Json {
    Null,
    Bool(bool),
    Int(JsonInt),
    Float(f64),
    Str(String),
    Array(Vec<Json>),
    Object(JsonObject),
}

/// An object with Python `dict` semantics: insertion order, replace in place, append new.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct JsonObject(Vec<(String, Json)>);

impl JsonObject {
    pub fn new() -> Self {
        Self(Vec::new())
    }

    /// Entries in order. The caller guarantees that keys are unique.
    pub fn from_entries(entries: Vec<(String, Json)>) -> Self {
        Self(entries)
    }

    pub fn entries(&self) -> &[(String, Json)] {
        &self.0
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn contains_key(&self, key: &str) -> bool {
        self.0.iter().any(|(name, _)| name == key)
    }

    pub fn get(&self, key: &str) -> Option<&Json> {
        self.0
            .iter()
            .find(|(name, _)| name == key)
            .map(|(_, value)| value)
    }

    pub fn get_mut(&mut self, key: &str) -> Option<&mut Json> {
        self.0
            .iter_mut()
            .find(|(name, _)| name == key)
            .map(|(_, value)| value)
    }

    /// Python `d[key] = value`: replaces in place, or appends a new key.
    pub fn insert(&mut self, key: &str, value: Json) {
        match self.get_mut(key) {
            Some(slot) => *slot = value,
            None => self.0.push((key.to_string(), value)),
        }
    }

    /// Python `d.setdefault(key, value)`: appends only when the key is absent.
    pub fn set_default(&mut self, key: &str, value: Json) {
        if !self.contains_key(key) {
            self.0.push((key.to_string(), value));
        }
    }

    /// Python `d.pop(key, None)`.
    pub fn remove(&mut self, key: &str) -> Option<Json> {
        let index = self.0.iter().position(|(name, _)| name == key)?;
        Some(self.0.remove(index).1)
    }

    pub fn keys(&self) -> impl Iterator<Item = &str> {
        self.0.iter().map(|(name, _)| name.as_str())
    }

    pub fn iter(&self) -> impl Iterator<Item = (&str, &Json)> {
        self.0.iter().map(|(name, value)| (name.as_str(), value))
    }
}

/// Whether any container sits more than `limit` levels deep, counting `level` containers
/// already around the starting values. Iterative, so it is safe on any input.
fn nests_beyond<'a>(start: impl Iterator<Item = &'a Json>, level: usize, limit: usize) -> bool {
    let mut pending: Vec<(&Json, usize)> = start.map(|value| (value, level)).collect();
    while let Some((value, around)) = pending.pop() {
        match value {
            Json::Array(items) => {
                if around + 1 > limit {
                    return true;
                }
                pending.extend(items.iter().map(|item| (item, around + 1)));
            }
            Json::Object(map) => {
                if around + 1 > limit {
                    return true;
                }
                pending.extend(map.iter().map(|(_, item)| (item, around + 1)));
            }
            _ => {}
        }
    }
    false
}

impl JsonObject {
    /// Whether this object, counted as one level, nests containers beyond `limit` levels.
    pub fn nests_beyond(&self, limit: usize) -> bool {
        limit == 0 || nests_beyond(self.iter().map(|(_, value)| value), 1, limit)
    }
}

impl Json {
    /// Whether the value nests containers beyond `limit` levels (a scalar has none).
    pub fn nests_beyond(&self, limit: usize) -> bool {
        nests_beyond(std::iter::once(self), 0, limit)
    }

    pub fn text(value: &str) -> Json {
        Json::Str(value.to_string())
    }

    pub fn int(value: i64) -> Json {
        Json::Int(JsonInt::from_i64(value))
    }

    pub fn texts(values: &[String]) -> Json {
        Json::Array(
            values
                .iter()
                .map(|value| Json::Str(value.clone()))
                .collect(),
        )
    }

    pub fn is_null(&self) -> bool {
        self == &Json::Null
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Json::Str(text) => Some(text),
            _ => None,
        }
    }

    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Json::Bool(flag) => Some(*flag),
            _ => None,
        }
    }

    pub fn as_int(&self) -> Option<&JsonInt> {
        match self {
            Json::Int(value) => Some(value),
            _ => None,
        }
    }

    pub fn as_array(&self) -> Option<&Vec<Json>> {
        match self {
            Json::Array(items) => Some(items),
            _ => None,
        }
    }

    pub fn as_array_mut(&mut self) -> Option<&mut Vec<Json>> {
        match self {
            Json::Array(items) => Some(items),
            _ => None,
        }
    }

    pub fn as_object(&self) -> Option<&JsonObject> {
        match self {
            Json::Object(map) => Some(map),
            _ => None,
        }
    }

    pub fn as_object_mut(&mut self) -> Option<&mut JsonObject> {
        match self {
            Json::Object(map) => Some(map),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
    use super::*;

    #[test]
    fn object_insert_replaces_in_place() {
        let mut map = JsonObject::new();
        map.insert("a", Json::int(1));
        map.insert("b", Json::int(2));
        map.insert("a", Json::int(3));
        let keys: Vec<&str> = map.keys().collect();
        assert_eq!(keys, ["a", "b"]);
        assert_eq!(map.get("a"), Some(&Json::int(3)));
    }

    #[test]
    fn nesting_is_counted_iteratively() {
        let mut value = Json::Null;
        for _ in 0..3 {
            value = Json::Array(Vec::from([value]));
        }
        assert!(!value.nests_beyond(3));
        assert!(value.nests_beyond(2));
        let object = JsonObject::from_entries(Vec::from([("a".to_string(), value)]));
        assert!(!object.nests_beyond(4));
        assert!(object.nests_beyond(3));
        assert!(!Json::Null.nests_beyond(0));
    }

    #[test]
    fn setdefault_appends_once() {
        let mut map = JsonObject::new();
        map.set_default("a", Json::int(1));
        map.set_default("a", Json::int(2));
        assert_eq!(map.len(), 1);
        assert_eq!(map.get("a"), Some(&Json::int(1)));
        assert_eq!(map.remove("a"), Some(Json::int(1)));
        assert!(map.is_empty());
    }
}
