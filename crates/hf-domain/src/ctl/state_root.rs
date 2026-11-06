//! A selected, lexically normalized state root. No filesystem meaning is implied.
pub const INVALID_ROOT_MESSAGE: &str = "Invalid state root: HARFLOW_STATE_ROOT must be a nonempty absolute path without NUL characters";
pub const INVALID_PATH_MESSAGE: &str = "Invalid state root: state paths must be valid nonempty filesystem paths without NUL characters";
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StateRoot(String);
impl StateRoot {
    /// The caller guarantees the text is already absolute and lexically normalized.
    pub fn from_normalized(text: String) -> Self {
        Self(text)
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InvalidStateRoot {
    pub message: &'static str,
}
