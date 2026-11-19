//! The durable capture-loss file: empty, or exactly one fixed line that names the cause.
use hf_domain::task_runtime::CaptureLoss;
use std::{io, path::Path};

const STALL_LINE: &[u8] = b"capture-known-loss stall\n";
const DISCONNECT_LINE: &[u8] = b"capture-known-loss disconnect\n";
/// Written before the cause was recorded; it never reads as a stall or a disconnect.
const LEGACY_LINE: &[u8] = b"capture-known-loss\n";
/// The classification of the bytes of a `loss.raw` file.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LossFile {
    /// Empty: no loss was recorded.
    None,
    Stall,
    Disconnect,
    /// The legacy line: a loss whose cause was not recorded.
    Unspecified,
    /// Any other content, or a file that cannot be read.
    UnknownCause,
}
/// The exact bytes recorded for a cause; empty for no loss.
pub fn loss_line(cause: CaptureLoss) -> &'static [u8] {
    match cause {
        CaptureLoss::None => b"",
        CaptureLoss::Stall => STALL_LINE,
        CaptureLoss::Disconnect => DISCONNECT_LINE,
    }
}
/// Classifies the complete contents of a loss file; only the exact lines are recognised.
pub fn classify_loss(bytes: &[u8]) -> LossFile {
    match bytes {
        b"" => LossFile::None,
        STALL_LINE => LossFile::Stall,
        DISCONNECT_LINE => LossFile::Disconnect,
        LEGACY_LINE => LossFile::Unspecified,
        _ => LossFile::UnknownCause,
    }
}
/// Reads and classifies a loss file by path. An absent or unreadable file is an unknown
/// cause for a reader; the writer never uses this to accept an absent file.
pub fn classify_loss_file(path: &Path) -> LossFile {
    const LIMIT: u64 = 64;
    let read = std::fs::File::open(path).and_then(|file| {
        use io::Read;
        let mut bytes = Vec::new();
        file.take(LIMIT).read_to_end(&mut bytes).map(|_| bytes)
    });
    read.map_or(LossFile::UnknownCause, |bytes| classify_loss(&bytes))
}
/// The classification the writer requires for a committed cause.
pub fn expected_file(cause: CaptureLoss) -> LossFile {
    match cause {
        CaptureLoss::None => LossFile::None,
        CaptureLoss::Stall => LossFile::Stall,
        CaptureLoss::Disconnect => LossFile::Disconnect,
    }
}
