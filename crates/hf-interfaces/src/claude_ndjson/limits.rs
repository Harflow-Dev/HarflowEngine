use hf_domain::backend::BackendError;
/// Provisional internal admission limits, not a qualified vendor production envelope.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limits {
    pub max_line_bytes: usize,
    pub max_event_bytes: usize,
    pub max_records: usize,
    pub max_depth: usize,
    pub max_identifier_bytes: usize,
    pub max_key_bytes: usize,
    pub max_collection_members: usize,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            max_line_bytes: 1 << 20,
            max_event_bytes: 1 << 20,
            max_records: 128,
            max_depth: 16,
            max_identifier_bytes: 256,
            max_key_bytes: 4096,
            max_collection_members: 128,
        }
    }
}
impl Limits {
    pub fn validate(self) -> Result<Self, BackendError> {
        let cap = Self::default();
        let pairs = [
            (self.max_line_bytes, cap.max_line_bytes),
            (self.max_event_bytes, cap.max_event_bytes),
            (self.max_records, cap.max_records),
            (self.max_depth, cap.max_depth),
            (self.max_identifier_bytes, cap.max_identifier_bytes),
            (self.max_key_bytes, cap.max_key_bytes),
            (self.max_collection_members, cap.max_collection_members),
        ];
        if pairs
            .iter()
            .any(|(value, ceiling)| *value == 0 || value > ceiling)
        {
            return Err(BackendError::Budget);
        }
        Ok(self)
    }
}
