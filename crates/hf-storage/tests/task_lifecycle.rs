#![cfg(unix)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

//! Atomic lifecycle evidence tests.
mod common;
mod fixture_support;
#[path = "task_lifecycle/mod.rs"]
mod lifecycle_tests;
mod task_writer_support;
