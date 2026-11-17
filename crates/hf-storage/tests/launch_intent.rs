#![cfg(unix)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

//! Actual private T0 intent and atomic public T2 materialization tests.
mod common;
mod fixture_support;
#[path = "launch_intent/mod.rs"]
mod launch_intent_tests;
mod task_writer_support;
