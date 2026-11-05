//! Strict bounded binary codec for private fixture channels only.
mod read;
mod write;
use hf_domain::fixture::Frame;
pub const MAX_FRAME: usize = 24576;
pub const MAX_ROOT: usize = 4096;
pub const MAX_PAYLOAD: usize = 16384;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CodecError {
    Length,
    Shape,
    Value,
}
pub fn encode(frame: &Frame) -> Result<Vec<u8>, CodecError> {
    write::encode(frame)
}
pub fn decode(bytes: &[u8]) -> Result<Frame, CodecError> {
    read::decode(bytes)
}
pub(super) fn canonical_path(path: &str) -> bool {
    path.starts_with('/')
        && path.len() <= MAX_ROOT
        && !path.contains('\0')
        && path != "/"
        && !path.ends_with('/')
        && path
            .split('/')
            .skip(1)
            .all(|p| !p.is_empty() && p != "." && p != "..")
}
