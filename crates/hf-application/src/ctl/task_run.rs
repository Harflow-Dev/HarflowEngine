//! Narrow first execution-observation admission values.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RunRequest {
    pub backend: String,
    pub freedom: String,
    pub repo: String,
    pub prompt: String,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RunFailure {
    Refused(String),
    Unknown(String),
}
