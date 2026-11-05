//! Internal exclusion port. Concrete OS handles and lock protocol live in adapters.
use std::{path::Path, time::Instant};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LeaseMode {
    Shared,
    Exclusive,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LeaseError {
    Busy,
    DeadlineExceeded,
    LimitExceeded,
    Invalid,
    Replaced,
    Unavailable,
}
/// Opaque lifetime token. Dropping it releases only its own local admission.
pub trait LeaseGuard: Send + Sync {}
pub trait LeasePort: Send + Sync {
    fn acquire(
        &self,
        path: &Path,
        mode: LeaseMode,
        create: bool,
        deadline: Instant,
    ) -> Result<Box<dyn LeaseGuard>, LeaseError>;
}
