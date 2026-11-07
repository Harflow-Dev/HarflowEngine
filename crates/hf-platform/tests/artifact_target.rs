#![cfg(all(target_os = "macos", target_arch = "aarch64"))]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
//! The observation of artifact targets against real files. The environment namespaces cannot
//! be varied here (the workspace forbids the unsafe call that sets a variable); the binary
//! tests in `tests/test_ctl_artifacts_authority.py` cover them.
use hf_application::ctl::task_reads::ArtifactTargetPort;
use hf_domain::ctl::history::{ArtifactItem, Availability, Observation};
use hf_platform::ctl::artifact_target::CommandArtifactTargets;
use std::{
    fs,
    os::unix::fs::{MetadataExt, PermissionsExt, symlink},
    path::{Path, PathBuf},
};

struct Scratch(PathBuf);

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = restore(&self.0);
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn restore(path: &Path) -> std::io::Result<()> {
    if path.is_dir() && !path.is_symlink() {
        fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
        for entry in fs::read_dir(path)? {
            restore(&entry?.path())?;
        }
    }
    Ok(())
}

fn scratch(name: &str) -> Scratch {
    let base =
        std::env::temp_dir().join(format!("hf-artifact-target-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&base);
    fs::create_dir_all(base.join("repo/out")).unwrap();
    Scratch(fs::canonicalize(&base).unwrap())
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

fn item(path: &Path, registered: Observation, method: &str) -> ArtifactItem {
    ArtifactItem {
        artifact_id: "x".into(),
        path: path.to_str().unwrap().into(),
        kind: "report".into(),
        note: String::new(),
        registered_at: "2026-01-01T00:00:00+00:00".into(),
        method: method.into(),
        registered,
    }
}

fn repo(scratch: &Scratch) -> String {
    scratch.0.join("repo").to_str().unwrap().into()
}

fn file(scratch: &Scratch, relative: &str) -> PathBuf {
    let path = scratch.0.join("repo").join(relative);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&path, b"artifact bytes").unwrap();
    path
}

fn state(scratch: &Scratch, path: &Path) -> Availability {
    CommandArtifactTargets::new().availability(
        &repo(scratch),
        &item(path, observed(path), "verified_process_lineage"),
    )
}

#[test]
fn an_untouched_regular_file_is_available_and_a_changed_one_is_changed() {
    let s = scratch("plain");
    let path = file(&s, "out/a.txt");
    let registered = observed(&path);
    let seen = |registered| {
        CommandArtifactTargets::new().availability(
            &repo(&s),
            &item(&path, registered, "verified_process_lineage"),
        )
    };
    assert_eq!(seen(registered), Availability::Available);
    for changed in [
        Observation {
            device: registered.device + 1,
            ..registered
        },
        Observation {
            inode: registered.inode + 1,
            ..registered
        },
        Observation {
            size: registered.size + 1,
            ..registered
        },
        Observation {
            mtime_ns: registered.mtime_ns + 1,
            ..registered
        },
    ] {
        assert_eq!(seen(changed), Availability::Changed, "{changed:?}");
    }
}

#[test]
fn a_missing_file_or_parent_is_deleted_even_when_the_repository_is_missing_too() {
    let s = scratch("gone");
    let path = file(&s, "out/a.txt");
    let registered = observed(&path);
    fs::remove_file(&path).unwrap();
    let at = |repo: &str, path: &Path| {
        CommandArtifactTargets::new()
            .availability(repo, &item(path, registered, "verified_process_lineage"))
    };
    assert_eq!(at(&repo(&s), &path), Availability::Deleted);
    fs::remove_dir(s.0.join("repo/out")).unwrap();
    assert_eq!(at(&repo(&s), &path), Availability::Deleted);
    assert_eq!(
        at("/nonexistent-harflow-repo", &path),
        Availability::Deleted,
        "deletion needs no authority"
    );
}

#[test]
fn a_symlink_or_a_symlinked_parent_is_changed_and_a_dangling_one_is_deleted() {
    let s = scratch("links");
    let path = file(&s, "out/a.txt");
    let registered = observed(&path);
    let at = |p: &Path| {
        CommandArtifactTargets::new()
            .availability(&repo(&s), &item(p, registered, "verified_process_lineage"))
    };
    let link = s.0.join("repo/link.txt");
    symlink(&path, &link).unwrap();
    assert_eq!(at(&link), Availability::Changed);
    let directory = s.0.join("repo/viewed");
    symlink(s.0.join("repo/out"), &directory).unwrap();
    assert_eq!(at(&directory.join("a.txt")), Availability::Changed);
    let dangling = s.0.join("repo/dangling");
    symlink(s.0.join("repo/nowhere"), &dangling).unwrap();
    assert_eq!(at(&dangling), Availability::Deleted);
    // The parent of the registered path replaced by a link after registration.
    let moved = s.0.join("repo/out-real");
    fs::rename(s.0.join("repo/out"), &moved).unwrap();
    symlink(&moved, s.0.join("repo/out")).unwrap();
    assert_eq!(at(&path), Availability::Changed);
}

#[test]
fn a_fifo_a_directory_and_an_unreadable_file_are_unavailable_without_blocking() {
    let s = scratch("special");
    let path = file(&s, "out/a.txt");
    let registered = observed(&path);
    let at = |p: &Path| {
        CommandArtifactTargets::new()
            .availability(&repo(&s), &item(p, registered, "verified_process_lineage"))
    };
    fs::remove_file(&path).unwrap();
    let status = std::process::Command::new("/usr/bin/mkfifo")
        .arg(&path)
        .status()
        .unwrap();
    assert!(status.success());
    assert_eq!(at(&path), Availability::Unavailable);
    fs::remove_file(&path).unwrap();
    fs::create_dir(&path).unwrap();
    assert_eq!(at(&path), Availability::Unavailable);
    fs::remove_dir(&path).unwrap();
    fs::write(&path, b"artifact bytes").unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o000)).unwrap();
    if !ignores_modes(&s) {
        assert_eq!(at(&path), Availability::Unavailable);
    }
    fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
}

/// Whether a mode 0 directory still opens, as it does for a privileged user.
fn ignores_modes(scratch: &Scratch) -> bool {
    let probe = scratch.0.join("probe");
    fs::create_dir_all(&probe).unwrap();
    fs::set_permissions(&probe, fs::Permissions::from_mode(0o000)).unwrap();
    let open = fs::read_dir(&probe).is_ok();
    fs::set_permissions(&probe, fs::Permissions::from_mode(0o700)).unwrap();
    open
}

#[test]
fn a_file_in_an_unsearchable_directory_is_unavailable() {
    let s = scratch("closed");
    let path = file(&s, "out/a.txt");
    let registered = observed(&path);
    fs::set_permissions(s.0.join("repo/out"), fs::Permissions::from_mode(0o000)).unwrap();
    if !ignores_modes(&s) {
        let seen = CommandArtifactTargets::new().availability(
            &repo(&s),
            &item(&path, registered, "verified_process_lineage"),
        );
        assert_eq!(seen, Availability::Unavailable);
    }
}

#[test]
fn a_target_outside_the_repository_or_in_a_sibling_is_unavailable() {
    let s = scratch("outside");
    let outside = s.0.join("outside.txt");
    fs::write(&outside, b"x").unwrap();
    assert_eq!(state(&s, &outside), Availability::Unavailable);
    let sibling = s.0.join("repo-twin");
    fs::create_dir_all(&sibling).unwrap();
    fs::write(sibling.join("t.txt"), b"x").unwrap();
    assert_eq!(state(&s, &sibling.join("t.txt")), Availability::Unavailable);
    assert_eq!(state(&s, &file(&s, "plain.txt")), Availability::Available);
}

#[test]
fn forbidden_names_anywhere_in_the_path_are_unavailable() {
    let s = scratch("names");
    for name in [
        ".harflow",
        concat!(".poly", "bridge"),
        ".codex",
        ".claude",
        ".vibe",
        ".config",
        ".git",
    ] {
        assert_eq!(
            state(&s, &file(&s, &format!("out/{name}"))),
            Availability::Unavailable,
            "file {name}"
        );
        assert_eq!(
            state(&s, &file(&s, &format!("{name}/inner.txt"))),
            Availability::Unavailable,
            "directory {name}"
        );
        assert_eq!(
            state(&s, &file(&s, &format!("deep/{name}/more/inner.txt"))),
            Availability::Unavailable,
            "deep {name}"
        );
    }
    for name in [".gitignore", ".github", "x.git", ".claude-notes", "config"] {
        assert_eq!(
            state(&s, &file(&s, &format!("out/{name}"))),
            Availability::Available,
            "{name}"
        );
    }
}

#[test]
fn project_namespaces_apply_at_the_repository_root_only() {
    let s = scratch("project");
    for name in [".mcp.json", "opencode.json", "opencode.jsonc"] {
        assert_eq!(
            state(&s, &file(&s, name)),
            Availability::Unavailable,
            "{name}"
        );
        assert_eq!(
            state(&s, &file(&s, &format!("sub/{name}"))),
            Availability::Available,
            "sub/{name}"
        );
        assert_eq!(
            state(&s, &file(&s, &format!("out/{name}"))),
            Availability::Available,
            "out/{name}"
        );
    }
    for name in [
        ".opencode",
        ".gemini",
        ".antigravity",
        ".cursor",
        ".agents/skills",
    ] {
        assert_eq!(
            state(&s, &file(&s, &format!("{name}/x.txt"))),
            Availability::Unavailable,
            "{name}"
        );
        assert_eq!(
            state(&s, &file(&s, &format!("sub/{name}/x.txt"))),
            Availability::Available,
            "sub/{name}"
        );
    }
}

#[test]
fn a_repository_below_a_project_namespace_directory_is_unavailable() {
    let s = scratch("below");
    for name in [
        ".opencode",
        ".gemini",
        ".antigravity",
        ".cursor",
        ".agents/skills",
    ] {
        let nested = s.0.join("holder").join(name).join("repo");
        fs::create_dir_all(&nested).unwrap();
        let path = nested.join("f.txt");
        fs::write(&path, b"x").unwrap();
        let seen = CommandArtifactTargets::new().availability(
            nested.to_str().unwrap(),
            &item(&path, observed(&path), "verified_process_lineage"),
        );
        assert_eq!(seen, Availability::Unavailable, "{name}");
    }
}

#[test]
fn the_provenance_this_engine_cannot_judge_is_unavailable_even_for_a_present_file() {
    let s = scratch("methods");
    let path = file(&s, "out/a.txt");
    for method in ["engine_output", "observed_backend_plan", "other", ""] {
        let seen = CommandArtifactTargets::new()
            .availability(&repo(&s), &item(&path, observed(&path), method));
        assert_eq!(seen, Availability::Unavailable, "{method:?}");
    }
}

#[test]
fn a_registered_path_in_another_case_is_changed() {
    let s = scratch("case");
    let path = file(&s, "out/a.txt");
    if !s.0.join("repo/OUT").exists() {
        return;
    }
    let upper = s.0.join("repo/out/A.TXT");
    let seen = CommandArtifactTargets::new().availability(
        &repo(&s),
        &item(&upper, observed(&path), "verified_process_lineage"),
    );
    assert_eq!(
        seen,
        Availability::Changed,
        "the kernel's spelling is the canonical one"
    );
}

#[test]
fn observing_changes_neither_the_file_nor_its_directory() {
    let s = scratch("quiet");
    let path = file(&s, "out/a.txt");
    let before = (
        fs::metadata(&path).unwrap().mtime_nsec(),
        fs::metadata(s.0.join("repo/out")).unwrap().mtime_nsec(),
    );
    for _ in 0..3 {
        assert_eq!(state(&s, &path), Availability::Available);
    }
    let after = (
        fs::metadata(&path).unwrap().mtime_nsec(),
        fs::metadata(s.0.join("repo/out")).unwrap().mtime_nsec(),
    );
    assert_eq!(before, after);
    assert_eq!(fs::read(&path).unwrap(), b"artifact bytes");
    assert_eq!(fs::read_dir(s.0.join("repo/out")).unwrap().count(), 1);
}
