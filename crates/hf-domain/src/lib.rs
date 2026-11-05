//! Pure bootstrap values. No runtime or wire compatibility is implied.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Readiness {
    Unsupported,
}

pub mod task_read;

pub mod event_read;

pub mod process;

pub mod ownership;

pub mod fixture;

pub mod task_watch;

pub mod capture;
