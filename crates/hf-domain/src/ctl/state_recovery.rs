//! The state-recovery record `store.recovery`: the Rust engine's recovery gate and commit
//! signal.
//!
//! Content: `harflow-recovery-1\n<operation_id>\n<pending|committed>\n`, at most
//! `RECORD_BOUND` bytes, where the operation id is 32 lowercase hexadecimal digits.
//! `pending` closes the root to every store opener; `committed` names the operation whose
//! recovery passed its commit point. The record is not bound to a store generation.

/// File name of the record in the state root.
pub const RECORD_NAME: &str = "store.recovery";
/// Upper bound on the record's length in bytes.
pub const RECORD_BOUND: u64 = 128;
const HEADER: &str = "harflow-recovery-1";
const OPERATION_LEN: usize = 32;

/// The recovery state the record names.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RecoveryState {
    /// A recovery has started and not committed: every store opener refuses.
    Pending,
    /// The recovery passed its commit point.
    Committed,
}
impl RecoveryState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Committed => "committed",
        }
    }
}

/// A decoded `store.recovery` record.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StoreRecovery {
    operation_id: String,
    state: RecoveryState,
}
impl StoreRecovery {
    /// `None` unless `operation_id` is 32 lowercase hexadecimal digits.
    pub fn new(operation_id: &str, state: RecoveryState) -> Option<Self> {
        is_operation_id(operation_id).then(|| Self {
            operation_id: operation_id.to_string(),
            state,
        })
    }
    pub fn operation_id(&self) -> &str {
        &self.operation_id
    }
    pub fn state(&self) -> RecoveryState {
        self.state
    }
    pub fn is_pending(&self) -> bool {
        self.state == RecoveryState::Pending
    }
    /// Whether this is the `committed` record of `operation_id`.
    pub fn commits(&self, operation_id: &str) -> bool {
        self.state == RecoveryState::Committed && self.operation_id == operation_id
    }
    /// The exact file content.
    pub fn encode(&self) -> String {
        let mut text = String::from(HEADER);
        text.push('\n');
        text.push_str(&self.operation_id);
        text.push('\n');
        text.push_str(self.state.as_str());
        text.push('\n');
        text
    }
    /// Decodes the exact content `encode` produces; anything else, including content over
    /// `RECORD_BOUND` bytes, is `None`.
    pub fn decode(bytes: &[u8]) -> Option<Self> {
        if bytes.len() as u64 > RECORD_BOUND {
            return None;
        }
        let text = String::from_utf8(bytes.to_vec()).ok()?;
        let rest = text.strip_prefix(HEADER)?.strip_prefix('\n')?;
        let (operation_id, rest) = rest.split_once('\n')?;
        let state = match rest.strip_suffix('\n')? {
            "pending" => RecoveryState::Pending,
            "committed" => RecoveryState::Committed,
            _ => return None,
        };
        Self::new(operation_id, state)
    }
}

/// Whether `text` is 32 lowercase hexadecimal digits, the reference's operation id form.
pub fn is_operation_id(text: &str) -> bool {
    text.len() == OPERATION_LEN
        && text
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

#[cfg(test)]
mod tests {
    use super::{RecoveryState, StoreRecovery};

    const OP: &str = "0123456789abcdef0123456789abcdef";

    #[test]
    fn records_round_trip() {
        for state in [RecoveryState::Pending, RecoveryState::Committed] {
            let record = StoreRecovery::new(OP, state);
            let encoded = record.as_ref().map(StoreRecovery::encode);
            let decoded = encoded.and_then(|text| StoreRecovery::decode(text.as_bytes()));
            assert_eq!(decoded, record);
        }
    }

    #[test]
    fn malformed_records_do_not_decode() {
        for text in [
            "",
            "harflow-recovery-1\n0123456789abcdef0123456789abcdef\npending",
            "harflow-recovery-1\n0123456789ABCDEF0123456789abcdef\npending\n",
            "harflow-recovery-1\n0123456789abcdef\npending\n",
            "harflow-recovery-1\n0123456789abcdef0123456789abcdef\ndone\n",
            "harflow-recovery-2\n0123456789abcdef0123456789abcdef\npending\n",
            "harflow-recovery-1\n0123456789abcdef0123456789abcdef\npending\n\n",
        ] {
            assert_eq!(StoreRecovery::decode(text.as_bytes()), None, "{text:?}");
        }
    }
}
