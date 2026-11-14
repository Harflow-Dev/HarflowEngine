#![cfg(all(target_os = "macos", target_arch = "aarch64"))]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, dead_code)]
//! Write latency and throughput measurements of the commit path with the commit watermark,
//! per durability mode, with the production lease. Every test is `#[ignore]` and reads its
//! settings from the environment: `HF_MEASURE_DIR` holds the dataset roots (from the
//! hf-storage `read_measure` generator), `HF_MEASURE_OUT` receives raw JSONL.
#[path = "write_latency/series.rs"]
mod series;
#[path = "write_latency/support.rs"]
mod support;
#[path = "write_latency/throughput.rs"]
mod throughput;

use hf_platform::durability::{Durability, port};
use series::Series;
use std::{fs::File, io::Write, path::PathBuf};
use support::{env, env_or, out_file};

const DEFAULT_SERIES: &str = "create/fsync/suppressed,create/full/suppressed,create/fsync/owner,create/full/owner,create_append/fsync/suppressed,create_append/full/suppressed";

/// The re-executed side of the child-mode and contention runs.
#[test]
#[ignore]
fn child() {
    match env_or("HF_WL_OP", "").as_str() {
        "operation" => series::child_operation(),
        "contend" => series::child_contend(),
        _ => {}
    }
}

/// Child-mode series, interleaved; `HF_WL_SERIES` lists `shape/mode/policy` entries.
#[test]
#[ignore]
fn latency() {
    let list: Vec<Series> = env_or("HF_WL_SERIES", DEFAULT_SERIES)
        .split(',')
        .map(Series::parse)
        .collect();
    series::run_child_series(&env("HF_MEASURE_LABEL"), &list);
}

/// In-process per-commit series: one long-lived Suppressed handle per mode.
#[test]
#[ignore]
fn in_process() {
    series::run_in_process(&env("HF_MEASURE_LABEL"), &["fsync", "full"]);
}

/// Four writer processes, 200 KB appends, FullFlush, under each listed policy.
#[test]
#[ignore]
fn contention() {
    let label = env("HF_MEASURE_LABEL");
    for policy in env_or("HF_WL_POLICIES", "owner,suppressed").split(',') {
        series::run_contention(&label, &env_or("HF_WL_MODE", "full"), policy);
    }
}

/// Burst profiles: each listed profile replayed by four concurrent tasks, then the
/// sustained profile (4 x 42 lines/s for 60 s), per listed mode.
#[test]
#[ignore]
fn throughput() {
    let label = env("HF_MEASURE_LABEL");
    let profiles: Vec<PathBuf> = env_or("HF_WL_PROFILES", &default_profiles())
        .split(',')
        .map(PathBuf::from)
        .collect();
    let reps = support::number("HF_WL_REPEAT", 3);
    for mode in env_or("HF_WL_MODES", "full,fsync").split(',') {
        for path in &profiles {
            let profile = throughput::read_profile(path);
            let stem = path.file_stem().unwrap().to_string_lossy().into_owned();
            let schedules = (0..4)
                .map(|_| throughput::burst(&profile, reps, 1000))
                .collect();
            throughput::run(&format!("{label}-burst-{stem}-{mode}"), mode, schedules);
        }
        let sizes: Vec<usize> = throughput::read_profile(&profiles[0])
            .into_iter()
            .map(|p| p.1)
            .collect();
        let seconds = support::number("HF_WL_SUSTAINED_S", 60);
        let schedules = (0..4)
            .map(|k| throughput::sustained(&sizes, 42, seconds, k * 6000))
            .collect();
        throughput::run(&format!("{label}-sustained-{mode}"), mode, schedules);
    }
}

/// The checked-in r06 and r07 profiles (`t_ms len` per line of the scrubbed recordings).
fn default_profiles() -> String {
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/write_latency/profiles");
    format!("{dir}/r06.txt,{dir}/r07.txt")
}

/// The maximum sustained commit rate of one writer over 30 s, per listed mode.
#[test]
#[ignore]
fn max_rate() {
    let label = env("HF_MEASURE_LABEL");
    for mode in env_or("HF_WL_MODES", "full,fsync").split(',') {
        throughput::max_rate(&format!("{label}-maxrate-{mode}"), mode, 30);
    }
}

fn flush_result(file: &File) -> String {
    match port(Durability::FullFlush).flush(file) {
        Ok(()) => "ok".into(),
        Err(error) => format!("errno {:?} {error}", error.raw_os_error()),
    }
}

/// `F_FULLFSYNC` through the platform port on a regular file of the measurement volume
/// (read-write and read-only descriptors), on `/dev/null` and on a pipe.
#[test]
#[ignore]
fn full_flush_check() {
    let path = support::dir().join("fullfsync-check");
    let mut file = File::create(&path).unwrap();
    file.write_all(b"check").unwrap();
    let pipe = std::io::pipe().unwrap();
    let pipe_file = File::from(std::os::fd::OwnedFd::from(pipe.1));
    let cases = [
        ("regular-rw", flush_result(&file)),
        ("regular-ro", flush_result(&File::open(&path).unwrap())),
        ("dev-null", flush_result(&File::open("/dev/null").unwrap())),
        ("pipe", flush_result(&pipe_file)),
    ];
    let mut out = out_file("fullfsync.jsonl");
    for (case, result) in cases {
        writeln!(out, r#"{{"case":"{case}","result":"{result}"}}"#).unwrap();
        println!("{case}: {result}");
    }
    let _ = std::fs::remove_file(&path);
}
