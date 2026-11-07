//! Strict access to one JSON object: unknown keys and missing required keys are refused.
use super::PortError;
use serde_json::{Map, Value};
pub(super) struct Args {
    object: Map<String, Value>,
    context: &'static str,
}
impl Args {
    pub(super) fn new(
        value: Value,
        context: &'static str,
        allowed: &[&str],
        required: &[&str],
    ) -> Result<Self, PortError> {
        let Value::Object(object) = value else {
            return Err(PortError::new(format!("{context} must be a JSON object")));
        };
        if let Some(key) = object.keys().find(|key| !allowed.contains(&key.as_str())) {
            return Err(PortError::new(format!("{context}: unknown key {key:?}")));
        }
        if let Some(key) = required.iter().find(|key| !object.contains_key(**key)) {
            return Err(PortError::new(format!("{context}: missing key {key:?}")));
        }
        Ok(Self { object, context })
    }
    fn wrong(&self, key: &str, expected: &str) -> PortError {
        PortError::new(format!("{}: {key:?} must be {expected}", self.context))
    }
    pub(super) fn has(&self, key: &str) -> bool {
        self.object.contains_key(key)
    }
    pub(super) fn string(&self, key: &str) -> Result<String, PortError> {
        match self.object.get(key) {
            Some(Value::String(text)) => Ok(text.clone()),
            _ => Err(self.wrong(key, "a string")),
        }
    }
    pub(super) fn opt_string(&self, key: &str) -> Result<Option<String>, PortError> {
        match self.object.get(key) {
            None | Some(Value::Null) => Ok(None),
            Some(Value::String(text)) => Ok(Some(text.clone())),
            _ => Err(self.wrong(key, "a string or null")),
        }
    }
    pub(super) fn u64(&self, key: &str) -> Result<u64, PortError> {
        self.object
            .get(key)
            .and_then(Value::as_u64)
            .ok_or_else(|| self.wrong(key, "a non-negative integer"))
    }
    pub(super) fn opt_i64(&self, key: &str) -> Result<Option<i64>, PortError> {
        match self.object.get(key) {
            None | Some(Value::Null) => Ok(None),
            Some(value) => value
                .as_i64()
                .map(Some)
                .ok_or_else(|| self.wrong(key, "an integer or null")),
        }
    }
    pub(super) fn bool(&self, key: &str) -> Result<bool, PortError> {
        match self.object.get(key) {
            Some(Value::Bool(flag)) => Ok(*flag),
            _ => Err(self.wrong(key, "a boolean")),
        }
    }
    pub(super) fn opt_bool(&self, key: &str) -> Result<Option<bool>, PortError> {
        match self.object.get(key) {
            None => Ok(None),
            Some(Value::Bool(flag)) => Ok(Some(*flag)),
            _ => Err(self.wrong(key, "a boolean")),
        }
    }
    pub(super) fn object(&self, key: &str) -> Result<&Map<String, Value>, PortError> {
        match self.object.get(key) {
            Some(Value::Object(map)) => Ok(map),
            _ => Err(self.wrong(key, "an object")),
        }
    }
    pub(super) fn object_value(&self, key: &str) -> Result<Value, PortError> {
        self.object(key).map(|map| Value::Object(map.clone()))
    }
    pub(super) fn array(&self, key: &str) -> Result<&Vec<Value>, PortError> {
        match self.object.get(key) {
            Some(Value::Array(items)) => Ok(items),
            _ => Err(self.wrong(key, "an array")),
        }
    }
    pub(super) fn take(&mut self, key: &str) -> Value {
        self.object.remove(key).unwrap_or(Value::Null)
    }
}
