#![cfg(all(target_os = "macos", target_arch = "aarch64"))]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
//! Lookup errors and the kernel's spelling of an artifact path, against real files. The
//! variable-driven namespaces are covered by the binary tests.
use hf_application::ctl::task_reads::ArtifactTargetPort;
use hf_domain::ctl::history::{ArtifactItem, Availability, Observation};
use hf_platform::ctl::artifact_target::CommandArtifactTargets;
use std::{
    fs,
    os::unix::fs::MetadataExt,
    path::{Path, PathBuf},
};

struct Scratch(PathBuf);

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn scratch(name: &str) -> Scratch {
    let base = std::env::temp_dir().join(format!(
        "hf-artifact-spelling-{name}-{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&base);
    fs::create_dir_all(base.join("repo/out")).unwrap();
    Scratch(fs::canonicalize(&base).unwrap())
}

fn item(path: &Path, registered: Observation) -> ArtifactItem {
    ArtifactItem {
        artifact_id: "x".into(),
        path: path.to_str().unwrap().into(),
        kind: "report".into(),
        note: String::new(),
        registered_at: "2026-01-01T00:00:00+00:00".into(),
        method: "verified_process_lineage".into(),
        plan: None,
        registered,
    }
}

fn observed(path: &Path) -> Observation {
    let info = fs::metadata(path).unwrap();
    Observation {
        device: info.dev(),
        inode: info.ino(),
        size: info.size(),
        mtime_ns: info.mtime() * 1_000_000_000 + info.mtime_nsec(),
    }
}

fn state(scratch: &Scratch, path: &Path, registered: Observation) -> Availability {
    CommandArtifactTargets::new().availability(
        scratch.0.join("repo").to_str().unwrap(),
        &item(path, registered),
    )
}

#[test]
fn a_firmlink_spelling_is_changed_when_the_data_volume_shows_the_path() {
    let s = scratch("firmlink");
    let path = s.0.join("repo/out/a.txt");
    fs::write(&path, b"x").unwrap();
    let registered = observed(&path);
    assert_eq!(state(&s, &path, registered), Availability::Available);
    let aliased = PathBuf::from("/System/Volumes/Data").join(path.strip_prefix("/").unwrap());
    if !aliased.exists() {
        return;
    }
    assert_eq!(state(&s, &aliased, registered), Availability::Changed);
}

#[test]
fn a_path_below_a_regular_file_is_unavailable_and_a_missing_one_is_deleted() {
    let s = scratch("notdir");
    let path = s.0.join("repo/out/a.txt");
    fs::write(&path, b"x").unwrap();
    let registered = observed(&path);
    assert_eq!(
        state(&s, &path.join("inner"), registered),
        Availability::Unavailable
    );
    assert_eq!(
        state(&s, &s.0.join("repo/out/gone.txt"), registered),
        Availability::Deleted
    );
}

#[test]
fn a_case_alias_of_a_protected_name_is_changed_and_never_reaches_the_authority_check() {
    let s = scratch("alias");
    let path = s.0.join("repo/out/.git");
    fs::write(&path, b"x").unwrap();
    let registered = observed(&path);
    assert_eq!(state(&s, &path, registered), Availability::Unavailable);
    if !s.0.join("repo/OUT").exists() {
        return;
    }
    assert_eq!(
        state(&s, &s.0.join("repo/out/.GIT"), registered),
        Availability::Changed
    );
}
