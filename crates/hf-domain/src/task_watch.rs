//! Pure internal task-list invalidation values.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WatchCursor {
    pub root: String,
    pub epoch: String,
    pub revision: i64,
}
impl WatchCursor {
    pub fn validate(&self) -> bool {
        valid_token(&self.root) && valid_token(&self.epoch) && self.revision >= 0
    }
}
fn valid_token(value: &str) -> bool {
    value.len() == 32
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WatchState {
    Baseline,
    Changed,
    Timeout,
    Reset,
    Unavailable,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WatchReason {
    EpochChanged,
    Discontinuity,
    StateMissing,
    ObservationUnavailable,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WatchResult {
    pub state: WatchState,
    pub cursor: Option<String>,
    pub reason: Option<WatchReason>,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WatchRequest {
    pub cursor: Option<String>,
    pub wait_seconds: u32,
}
