use super::{DecodeError, Value};
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Field<T> {
    Missing,
    Null,
    Value(T),
}
pub fn field<'a>(raw: &'a Value, name: &str) -> Field<&'a Value> {
    match raw.get(name) {
        None => Field::Missing,
        Some(Value::Null) => Field::Null,
        Some(value) => Field::Value(value),
    }
}
fn typed<'a, T>(
    raw: &'a Value,
    name: &str,
    convert: impl FnOnce(&'a Value) -> Option<T>,
) -> Result<Field<T>, DecodeError> {
    match field(raw, name) {
        Field::Missing => Ok(Field::Missing),
        Field::Null => Ok(Field::Null),
        Field::Value(value) => convert(value).map(Field::Value).ok_or(DecodeError::Field),
    }
}
#[derive(Clone, Copy, Debug)]
pub struct TaskView<'a>(pub(super) &'a Value);
impl<'a> TaskView<'a> {
    pub fn raw(self) -> &'a Value {
        self.0
    }
    pub fn task_id(self) -> Result<&'a str, DecodeError> {
        self.0
            .get("task_id")
            .and_then(Value::as_str)
            .ok_or(DecodeError::Field)
    }
    pub fn field(self, name: &str) -> Field<&'a Value> {
        field(self.0, name)
    }
    pub fn string(self, name: &str) -> Result<Field<&'a str>, DecodeError> {
        typed(self.0, name, Value::as_str)
    }
    pub fn boolean(self, name: &str) -> Result<Field<bool>, DecodeError> {
        typed(self.0, name, Value::as_bool)
    }
    pub fn unsigned(self, name: &str) -> Result<Field<u64>, DecodeError> {
        typed(self.0, name, Value::as_u64)
    }
}
#[derive(Clone, Copy, Debug)]
pub struct PageView<'a>(pub(super) &'a Value);
impl<'a> PageView<'a> {
    pub fn raw(self) -> &'a Value {
        self.0
    }
    pub fn field(self, name: &str) -> Field<&'a Value> {
        field(self.0, name)
    }
    pub fn boolean(self, name: &str) -> Result<Field<bool>, DecodeError> {
        typed(self.0, name, Value::as_bool)
    }
    pub fn unsigned(self, name: &str) -> Result<Field<u64>, DecodeError> {
        typed(self.0, name, Value::as_u64)
    }
    pub fn items(self) -> Result<Vec<TaskView<'a>>, DecodeError> {
        let items = self
            .0
            .get("items")
            .and_then(Value::as_array)
            .ok_or(DecodeError::Field)?;
        Ok(items.iter().map(TaskView).collect())
    }
    pub fn next_cursor(self) -> Result<Field<&'a str>, DecodeError> {
        typed(self.0, "next_cursor", Value::as_str)
    }
}
#[derive(Clone, Copy, Debug)]
pub struct ErrorView<'a>(pub(super) &'a Value);
impl<'a> ErrorView<'a> {
    pub fn raw(self) -> &'a Value {
        self.0
    }
    pub fn code(self) -> Result<&'a str, DecodeError> {
        self.0
            .get("code")
            .and_then(Value::as_str)
            .ok_or(DecodeError::Field)
    }
    pub fn message(self) -> Result<&'a str, DecodeError> {
        self.0
            .get("message")
            .and_then(Value::as_str)
            .ok_or(DecodeError::Field)
    }
    pub fn data(self) -> Field<&'a Value> {
        field(self.0, "data")
    }
}
pub(super) fn validate_error(raw: &Value) -> Result<(), DecodeError> {
    if !raw.is_object()
        || !raw.get("code").is_some_and(Value::is_string)
        || !raw.get("message").is_some_and(Value::is_string)
    {
        return Err(DecodeError::Payload);
    }
    Ok(())
}
pub(super) fn validate_task(raw: &Value) -> Result<(), DecodeError> {
    let id = raw
        .get("task_id")
        .and_then(Value::as_str)
        .ok_or(DecodeError::Payload)?;
    if !raw.is_object()
        || id.is_empty()
        || id.len() > 64
        || !id.as_bytes()[0].is_ascii_alphanumeric()
        || !id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
    {
        return Err(DecodeError::Payload);
    }
    let task = TaskView(raw);
    for name in [
        "status",
        "started_at",
        "finished_at",
        "workflow_run_id",
        "session_id",
        "output_capture",
        "input_channel",
        "last_captured_output_at",
    ] {
        task.string(name)?;
    }
    for name in [
        "workflow_builder",
        "active",
        "live_input",
        "observed_exit",
        "output_drained",
        "owned_by_live_server",
        "needs_reconciliation",
        "recovered",
        "status_reconciled",
    ] {
        task.boolean(name)?;
    }
    typed(raw, "exit_code", Value::as_i64)?;
    typed(raw, "background_idle_seconds", Value::as_f64)?;
    typed(raw, "supervision", Value::as_object)?;
    Ok(())
}
pub(super) fn validate_page(raw: &Value) -> Result<(), DecodeError> {
    let items = raw
        .get("items")
        .and_then(Value::as_array)
        .ok_or(DecodeError::Payload)?;
    if items.len() > 100 {
        return Err(DecodeError::Payload);
    }
    for item in items {
        validate_task(item)?;
    }
    for name in [
        "has_more",
        "bootstrap_pending",
        "counts_complete",
        "history_incomplete",
        "authority_incomplete",
        "ownership_incomplete",
        "scan_incomplete",
    ] {
        typed(raw, name, Value::as_bool)?;
    }
    for name in [
        "total",
        "active_count",
        "total_count",
        "total_active_count",
        "total_active_root_count",
        "total_attention_root_count",
    ] {
        typed(raw, name, Value::as_u64)?;
    }
    typed(raw, "catalog_state", Value::as_object)?;
    if let Field::Value(cursor) = typed(raw, "next_cursor", Value::as_str)?
        && cursor.chars().count() > 2048
    {
        return Err(DecodeError::Field);
    }
    Ok(())
}
