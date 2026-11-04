//! Bounded consumer observations and pure fixture preparation, without live adapters.
use crate::task_consumer::{DecodeError, Field, VersionLine};
use serde_json::Value;
mod preparation;
mod validation;
pub use preparation::{PreparedLine, prepare_line};

#[derive(Debug)]
pub struct Document {
    raw: Value,
}
impl Document {
    pub fn parse(bytes: &[u8], expected: VersionLine) -> Result<Self, DecodeError> {
        if bytes.len() > 262_144 {
            return Err(DecodeError::Size);
        }
        let raw: Value = serde_json::from_slice(bytes).map_err(|_| DecodeError::Json)?;
        crate::task_consumer::envelope(&raw, expected)?;
        if let Some(error) = raw.get("error") {
            if !error.is_object()
                || !error.get("code").is_some_and(Value::is_string)
                || !error.get("message").is_some_and(Value::is_string)
            {
                return Err(DecodeError::Payload);
            }
        } else {
            validation::page(raw.get("result").ok_or(DecodeError::Payload)?)?;
        }
        Ok(Self { raw })
    }
    pub fn raw(&self) -> &Value {
        &self.raw
    }
    pub fn page(&self) -> Option<View<'_>> {
        if self.raw.get("error").is_some() {
            None
        } else {
            self.raw.get("result").map(View)
        }
    }
    pub fn error(&self) -> Option<View<'_>> {
        self.raw.get("error").map(View)
    }
}
#[derive(Clone, Copy, Debug)]
pub struct View<'a>(&'a Value);
impl<'a> View<'a> {
    pub fn raw(self) -> &'a Value {
        self.0
    }
    pub fn field(self, key: &str) -> Field<&'a Value> {
        match self.0.get(key) {
            None => Field::Missing,
            Some(Value::Null) => Field::Null,
            Some(v) => Field::Value(v),
        }
    }
    pub fn events(self) -> Option<Vec<View<'a>>> {
        self.0
            .get("events")?
            .as_array()
            .map(|a| a.iter().map(View).collect())
    }
}
