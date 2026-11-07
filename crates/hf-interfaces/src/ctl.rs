//! Public ctl transport mapping: argv parsing, JSON documents and exit codes.
mod bound;
pub mod cli;
mod cli_scan;
pub mod cursor;
pub mod dispatch;
pub mod json_scan;
mod read_dispatch;
mod read_docs;
mod read_errors;
pub mod render;
pub mod state_codec;
mod task_docs;
