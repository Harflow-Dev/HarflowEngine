//! One error type: a message for standard error.
#[derive(Debug)]
pub struct Fail(String);
impl Fail {
    pub fn new(message: impl Into<String>) -> Self {
        Self(message.into())
    }
}
impl std::fmt::Display for Fail {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}
impl From<std::io::Error> for Fail {
    fn from(error: std::io::Error) -> Self {
        Self(format!("i/o error: {error}"))
    }
}
impl From<hf_storage::Error> for Fail {
    fn from(error: hf_storage::Error) -> Self {
        Self(format!("storage error: {error}"))
    }
}
impl From<hf_interfaces::fixture_port::PortError> for Fail {
    fn from(error: hf_interfaces::fixture_port::PortError) -> Self {
        Self(error.message().to_owned())
    }
}
pub type Result<T> = std::result::Result<T, Fail>;
