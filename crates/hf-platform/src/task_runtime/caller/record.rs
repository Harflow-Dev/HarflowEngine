//! Pure observational decoding; selecting a layout does not admit an OS reader.
use super::unknown;
use std::io;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CallerRecordLayout {
    /// SDK 26.5 layout, observed separately on macOS 26.6.2 arm64.
    MacOsArm64Sdk26_5,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CallerProcessRecord {
    pub pid: u32,
    pub parent: u32,
    pub uid: u32,
    pub status: u32,
    pub start_seconds: u64,
    pub start_microseconds: u64,
}
impl CallerProcessRecord {
    pub(super) fn validate(self, expected_pid: u32) -> io::Result<Self> {
        if self.pid != expected_pid
            || self.pid == 0
            || i32::try_from(self.pid).is_err()
            || i32::try_from(self.parent).is_err()
            || self.start_seconds == 0
            || self.start_microseconds >= 1_000_000
            || !(1..=4).contains(&self.status)
            || (self.pid == 1 && self.parent != 0)
            || (self.pid != 1 && self.parent == 0)
        {
            return Err(unknown());
        }
        Ok(self)
    }
}
fn bytes<const N: usize>(input: &[u8], offset: usize) -> io::Result<[u8; N]> {
    input
        .get(offset..offset + N)
        .ok_or_else(unknown)?
        .try_into()
        .map_err(|_| unknown())
}
/// Decode only the explicitly selected layout, with no allocation or native casts.
pub fn decode_caller_record(
    layout: CallerRecordLayout,
    expected_pid: u32,
    input: &[u8],
) -> io::Result<CallerProcessRecord> {
    match layout {
        CallerRecordLayout::MacOsArm64Sdk26_5 if input.len() == 648 => {}
        _ => return Err(unknown()),
    }
    let seconds = i64::from_le_bytes(bytes(input, 0)?);
    let micros = i32::from_le_bytes(bytes(input, 8)?);
    let status = i8::from_le_bytes(bytes(input, 36)?);
    let pid = i32::from_le_bytes(bytes(input, 40)?);
    let parent = i32::from_le_bytes(bytes(input, 560)?);
    CallerProcessRecord {
        pid: u32::try_from(pid).map_err(|_| unknown())?,
        parent: u32::try_from(parent).map_err(|_| unknown())?,
        uid: u32::from_le_bytes(bytes(input, 420)?),
        status: u32::try_from(status).map_err(|_| unknown())?,
        start_seconds: u64::try_from(seconds).map_err(|_| unknown())?,
        start_microseconds: u64::try_from(micros).map_err(|_| unknown())?,
    }
    .validate(expected_pid)
}
