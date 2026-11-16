//! Workflow definitions: the ports they need, validation and identity. Parsing, rendering
//! and every effect stay behind the ports.
pub mod diagnostics;
pub mod identity;
pub mod ports;
pub mod references;
pub mod validate;

pub mod catalog;
pub mod save;
mod save_defaults;
