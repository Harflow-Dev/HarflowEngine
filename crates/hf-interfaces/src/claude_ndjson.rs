//! Drainable internal observations, without execution, correlation or acceptance authority.
mod json;
mod limits;
mod normalize;
mod production;
use hf_domain::{
    backend::{BackendError, BackendRecord},
    ownership::Id,
};
pub use limits::Limits;
use production::production_boundary;
pub use production::{Boundary, line_boundary};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Outcome {
    NeedMore,
    Event(Vec<BackendRecord>),
    Fault(BackendError),
    Closed,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Feed {
    pub consumed: usize,
    pub outcome: Outcome,
}
/// Retains only one bounded partial line. The caller retains unconsumed bytes.
pub struct Decoder {
    attempt: Id,
    limits: Limits,
    pending: Vec<u8>,
    fault: Option<BackendError>,
    closed: bool,
    production: bool,
}
impl Decoder {
    pub fn new(attempt: Id, limits: Limits) -> Result<Self, BackendError> {
        Ok(Self {
            attempt,
            limits: limits.validate()?,
            pending: Vec::new(),
            fault: None,
            closed: false,
            production: false,
        })
    }
    /// Production parsing preserves fixture refusals while admitting actual CLI metadata.
    pub fn new_production(attempt: Id, limits: Limits) -> Result<Self, BackendError> {
        let mut decoder = Self::new(attempt, limits)?;
        decoder.production = true;
        Ok(decoder)
    }
    pub fn pending_bytes(&self) -> usize {
        self.pending.len()
    }
    pub fn feed(&mut self, input: &[u8]) -> Feed {
        if let Some(error) = self.fault {
            return Feed {
                consumed: 0,
                outcome: Outcome::Fault(error),
            };
        }
        if self.closed {
            return Feed {
                consumed: 0,
                outcome: Outcome::Closed,
            };
        }
        for (index, byte) in input.iter().take(4096).enumerate() {
            if *byte == b'\n' {
                let result = json::parse(&self.pending, self.limits).and_then(|v| {
                    normalize::records(&v, &self.attempt, self.limits, self.production)
                });
                self.pending.clear();
                return match result {
                    Ok(records) => Feed {
                        consumed: index + 1,
                        outcome: Outcome::Event(records),
                    },
                    Err(error) => self.fail(index + 1, error),
                };
            }
            if self.pending.len() == self.limits.max_line_bytes {
                return self.fail(index + 1, BackendError::Budget);
            }
            self.pending.push(*byte);
        }
        Feed {
            consumed: input.len().min(4096),
            outcome: Outcome::NeedMore,
        }
    }
    fn fail(&mut self, consumed: usize, error: BackendError) -> Feed {
        self.pending.clear();
        self.fault = Some(error);
        Feed {
            consumed,
            outcome: Outcome::Fault(error),
        }
    }
    /// Clean physical EOF is permitted without a terminal observation.
    pub fn finish(&mut self) -> Result<(), BackendError> {
        if let Some(error) = self.fault {
            return Err(error);
        }
        if !self.pending.is_empty() {
            self.fail(0, BackendError::Truncated);
            return Err(BackendError::Truncated);
        }
        self.closed = true;
        Ok(())
    }
}
