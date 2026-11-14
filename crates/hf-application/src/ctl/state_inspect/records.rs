//! Refusals from the engine-owned state files read after `-wal`: the lost-WAL
//! acceptance record, the verification fence and the commit watermark, in that order.
use crate::watermark::{self, Decoded};
use hf_domain::ctl::inspect::{FileState, InspectReason};

/// The lost-WAL acceptance record, after `-wal`: absent or `accepted` for this
/// generation passes; `pending`, `failed` and an unreadable record read as a missing
/// `-wal`; anything else is an invalid marker, as the read session refuses it.
pub(super) fn acceptance_refusal(record: &FileState, generation: &str) -> Option<InspectReason> {
    let bytes = match record {
        FileState::Absent => return None,
        FileState::Bytes(bytes) => bytes,
        FileState::Unreadable => return Some(InspectReason::InfrastructureUnavailable),
        FileState::NotRegular | FileState::Oversize => return Some(InspectReason::InvalidMarker),
    };
    let text = String::from_utf8_lossy(bytes);
    let prefix = format!("harflow-wal-acceptance-1\n{generation}\n");
    match text.strip_prefix(&prefix) {
        Some("accepted\n") => None,
        Some("pending\n" | "failed\n") => Some(InspectReason::InfrastructureUnavailable),
        _ => Some(InspectReason::InvalidMarker),
    }
}
/// The verification fence, after the acceptance record: absent passes; `failed` for
/// this generation or an unreadable fence refuses as unavailable; anything else is an
/// invalid marker, as the read session refuses it.
pub(super) fn fence_refusal(fence: &FileState, generation: &str) -> Option<InspectReason> {
    let bytes = match fence {
        FileState::Absent => return None,
        FileState::Bytes(bytes) => bytes,
        FileState::Unreadable => return Some(InspectReason::InfrastructureUnavailable),
        FileState::NotRegular | FileState::Oversize => return Some(InspectReason::InvalidMarker),
    };
    let text = String::from_utf8_lossy(bytes);
    match text.strip_prefix(&format!("harflow-fence-1\n{generation}\n")) {
        Some("failed\n") => Some(InspectReason::InfrastructureUnavailable),
        _ => Some(InspectReason::InvalidMarker),
    }
}
/// The commit watermark, after the fence: a valid slot for this generation passes; an
/// absent or unreadable file, or one with no valid slot, refuses as unavailable; a
/// non-regular file, a length other than 8192 bytes or a foreign generation is an invalid
/// marker, as the read session refuses it.
pub(super) fn watermark_refusal(record: &FileState, generation: &str) -> Option<InspectReason> {
    let bytes = match record {
        FileState::Absent | FileState::Unreadable => {
            return Some(InspectReason::InfrastructureUnavailable);
        }
        FileState::Bytes(bytes) => bytes,
        FileState::NotRegular | FileState::Oversize => return Some(InspectReason::InvalidMarker),
    };
    match watermark::decode(bytes, generation) {
        Decoded::Valid { .. } => None,
        Decoded::Unavailable => Some(InspectReason::InfrastructureUnavailable),
        Decoded::Incompatible => Some(InspectReason::InvalidMarker),
    }
}
