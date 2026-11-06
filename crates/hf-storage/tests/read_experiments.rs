#![cfg(unix)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, dead_code)]
//! Slice S2 read-path experiments (all `#[ignore]`): read-only open semantics,
//! corruption behaviour, killed readers and writer concurrency. See
//! docs/store-read-path.md.
mod common;
#[path = "read_experiments/corrupt.rs"]
mod corrupt;
#[path = "read_measure/data.rs"]
mod data;
#[path = "read_experiments/late.rs"]
mod late;
#[path = "read_measure/ops.rs"]
mod ops;
#[path = "read_experiments/process.rs"]
mod process;
#[path = "read_experiments/retention.rs"]
mod retention;
#[path = "read_experiments/sidecars.rs"]
mod sidecars;
#[path = "read_experiments/util.rs"]
mod util;

#[test]
#[ignore]
fn sidecar_semantics() {
    sidecars::SCENARIOS.iter().for_each(sidecars::run);
}

#[test]
#[ignore]
fn corruption() {
    corrupt::main_file();
    corrupt::wal_file();
    late::late_corruption();
}

#[test]
#[ignore]
fn killed_readers() {
    process::killed();
}

#[test]
#[ignore]
fn writer_safety() {
    process::writer_safety();
}

#[test]
#[ignore]
fn exp_child() {
    process::child();
}

#[test]
#[ignore]
fn wal_size_policy() {
    retention::wal_size_policy();
}

#[test]
#[ignore]
fn shm_retention() {
    retention::shm_retention();
}
