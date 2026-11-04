//! Consumer-only task observations. These checks confer no runtime authority.
//! Byte limits and serde_json's default recursion bound (128) are local parser limits,
//! not public command limits. Original JSON values are retained without rewriting.
use serde_json::Value;
mod views;
pub use views::{ErrorView, Field, PageView, TaskView};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VersionLine {
    major: u32,
    minor: u32,
}
impl VersionLine {
    pub fn new(major: u32, minor: u32) -> Result<Self, DecodeError> {
        if major > i32::MAX as u32 || minor > i32::MAX as u32 {
            return Err(DecodeError::Version);
        }
        Ok(Self { major, minor })
    }
}
#[derive(Clone, Copy, Debug)]
pub struct ParseLimits {
    pub max_bytes: usize,
}
impl Default for ParseLimits {
    fn default() -> Self {
        Self {
            max_bytes: 1024 * 1024,
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DecodeError {
    Size,
    Json,
    Envelope,
    Version,
    Payload,
    Field,
}
impl std::fmt::Display for DecodeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "task consumer decode: {self:?}")
    }
}
impl std::error::Error for DecodeError {}
#[derive(Debug)]
pub struct Document {
    raw: Value,
}
impl Document {
    pub fn parse(
        bytes: &[u8],
        expected: VersionLine,
        limits: ParseLimits,
    ) -> Result<Self, DecodeError> {
        if bytes.len() > limits.max_bytes {
            return Err(DecodeError::Size);
        }
        let raw: Value = serde_json::from_slice(bytes).map_err(|_| DecodeError::Json)?;
        envelope(&raw, expected)?;
        if let Some(error) = raw.get("error") {
            views::validate_error(error)?;
        } else if let Some(task) = raw.get("task") {
            views::validate_task(task)?;
        } else if let Some(page) = raw.get("result") {
            views::validate_page(page)?;
        } else {
            return Err(DecodeError::Payload);
        }
        Ok(Self { raw })
    }
    pub fn raw(&self) -> &Value {
        &self.raw
    }
    pub fn status(&self) -> Option<TaskView<'_>> {
        if self.error().is_some() {
            return None;
        }
        self.raw.get("task").map(TaskView)
    }
    pub fn page(&self) -> Option<PageView<'_>> {
        if self.error().is_some() || self.status().is_some() {
            return None;
        }
        self.raw.get("result").map(PageView)
    }
    pub fn error(&self) -> Option<ErrorView<'_>> {
        self.raw.get("error").map(ErrorView)
    }
}
pub(crate) fn envelope(raw: &Value, expected: VersionLine) -> Result<(), DecodeError> {
    if !raw.is_object()
        || raw.get("product").and_then(Value::as_str) != Some("harflow")
        || raw.get("format").and_then(Value::as_str) != Some("harflow.ctl")
        || raw.get("v").and_then(Value::as_u64) != Some(1)
    {
        return Err(DecodeError::Envelope);
    }
    let version = raw
        .get("product_version")
        .and_then(Value::as_str)
        .ok_or(DecodeError::Version)?;
    let (major, minor) = release(version).ok_or(DecodeError::Version)?;
    if (major, minor) != (expected.major, expected.minor) {
        return Err(DecodeError::Version);
    }
    Ok(())
}
pub(crate) fn release(text: &str) -> Option<(u32, u32)> {
    if text.len() > 256 || !text.is_ascii() {
        return None;
    }
    let mut halves = text.split('+');
    let base = halves.next()?;
    if let Some(build) = halves.next()
        && build.split('.').any(|part| {
            part.is_empty() || !part.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
        })
    {
        return None;
    }
    if halves.next().is_some() {
        return None;
    }
    let mut parts = base.split('.');
    let major = component(parts.next()?)?;
    let minor = component(parts.next()?)?;
    component(parts.next()?)?;
    if parts.next().is_some() {
        return None;
    }
    Some((major, minor))
}
fn component(text: &str) -> Option<u32> {
    if text.is_empty()
        || (text.len() > 1 && text.starts_with('0'))
        || !text.bytes().all(|b| b.is_ascii_digit())
    {
        return None;
    }
    text.parse::<u32>()
        .ok()
        .filter(|value| *value <= i32::MAX as u32)
}
