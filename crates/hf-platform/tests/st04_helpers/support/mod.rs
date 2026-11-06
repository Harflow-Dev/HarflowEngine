#![allow(clippy::expect_used, clippy::panic)]

mod build;
mod case;
mod evidence;
mod host;
mod report;

pub(crate) use build::{Build, sha256};
pub(crate) use case::{CaseResult, run_case};
pub(crate) use evidence::{evidence_dir, write_bytes};
pub(crate) use host::{file_identity, host_facts};
pub(crate) use report::{Manifest, json_string, write_case_report, write_manifest};

pub(crate) fn write_synced_bytes(path: &std::path::Path, bytes: &[u8]) {
    use std::io::Write;
    let mut file = std::fs::File::create(path).expect("create evidence file");
    file.write_all(bytes).expect("write evidence file");
    file.sync_all().expect("sync evidence file");
}
