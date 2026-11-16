//! Fresh operation identifiers from the operating system's random source.
use hf_application::ctl::state_inspect::OperationIdPort;

#[derive(Clone, Copy, Debug, Default)]
pub struct OsOperationIds;

impl OperationIdPort for OsOperationIds {
    /// 32 lowercase hexadecimal digits from 16 bytes of `/dev/urandom`; `None` when the
    /// source cannot be read.
    fn fresh(&self) -> Option<String> {
        use std::io::Read;
        let mut bytes = [0u8; 16];
        std::fs::File::open("/dev/urandom")
            .and_then(|mut source| source.read_exact(&mut bytes))
            .ok()?;
        let mut text = String::with_capacity(32);
        for byte in bytes {
            text.push(char::from(b"0123456789abcdef"[usize::from(byte >> 4)]));
            text.push(char::from(b"0123456789abcdef"[usize::from(byte & 15)]));
        }
        Some(text)
    }
}
