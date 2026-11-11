#![cfg(all(target_os = "macos", target_arch = "aarch64"))]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
//! One command's artifact observations: the account lookup that decides the opencode managed
//! preference path, injected so no test needs another user or a write under /Library.
//!
//! The account name is joined below `/Library/Managed Preferences`, so a name of the form
//! `../../<scratch>/acct` puts the "account directory" in a scratch area the test owns.
use hf_application::ctl::task_reads::ArtifactTargetPort;
use hf_domain::ctl::history::{ArtifactItem, Availability, Observation};
use hf_platform::ctl::artifact_target::CommandArtifactTargets;
use std::{
    cell::Cell,
    fs,
    os::unix::fs::{MetadataExt, symlink},
    path::{Path, PathBuf},
    rc::Rc,
};

const PLIST: &str = "ai.opencode.managed.plist";
const MESSAGE: &str = "'getpwuid(): uid not found: 4242'";

struct Scratch(PathBuf);

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn scratch(name: &str) -> Scratch {
    let base =
        std::env::temp_dir().join(format!("hf-artifact-command-{name}-{}", std::process::id()));
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
        plan: None,
        registered,
    }
}

fn file(scratch: &Scratch, relative: &str) -> PathBuf {
    let path = scratch.0.join("repo").join(relative);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&path, b"x").unwrap();
    path
}

fn repo(scratch: &Scratch) -> String {
    scratch.0.join("repo").to_str().unwrap().into()
}

/// An account name that puts its managed-preference directory at `scratch/relative`.
fn account_at(scratch: &Scratch, relative: &str) -> String {
    format!("../..{}", scratch.0.join(relative).to_str().unwrap())
}

fn counting(result: Result<String, String>) -> (CommandArtifactTargets, Rc<Cell<usize>>) {
    let calls = Rc::new(Cell::new(0));
    let seen = calls.clone();
    let targets = CommandArtifactTargets::with_account(move || {
        seen.set(seen.get() + 1);
        result.clone()
    });
    (targets, calls)
}

fn state(targets: &CommandArtifactTargets, scratch: &Scratch, path: &Path) -> Availability {
    targets.availability(
        &repo(scratch),
        &item(path, observed(path), "verified_process_lineage"),
    )
}

#[test]
fn a_failed_account_lookup_makes_the_artifact_unavailable_and_ends_the_command_with_the_references_message()
 {
    let s = scratch("failure");
    let (targets, calls) = counting(Err(MESSAGE.into()));
    assert_eq!(
        targets.command_failure(),
        None,
        "nothing has asked for the account yet"
    );
    let path = file(&s, "out/a.txt");
    assert_eq!(state(&targets, &s, &path), Availability::Unavailable);
    assert_eq!(targets.command_failure().as_deref(), Some(MESSAGE));
    let other = file(&s, "out/b.txt");
    assert_eq!(
        state(&targets, &s, &other),
        Availability::Unavailable,
        "every artifact of the command"
    );
    assert_eq!(calls.get(), 1, "the lookup is read once per command");
    assert_eq!(targets.command_failure().as_deref(), Some(MESSAGE));
}

#[test]
fn a_deleted_changed_protected_or_unjudgeable_artifact_never_asks_for_the_account() {
    let s = scratch("never");
    let (targets, calls) = counting(Err(MESSAGE.into()));
    let path = file(&s, "out/a.txt");
    let registered = observed(&path);
    let gone = s.0.join("repo/out/gone.txt");
    assert_eq!(
        targets.availability(
            &repo(&s),
            &item(&gone, registered, "verified_process_lineage")
        ),
        Availability::Deleted
    );
    let link = s.0.join("repo/link.txt");
    symlink(&path, &link).unwrap();
    assert_eq!(
        targets.availability(
            &repo(&s),
            &item(&link, registered, "verified_process_lineage")
        ),
        Availability::Changed
    );
    let named = file(&s, "out/.git");
    assert_eq!(state(&targets, &s, &named), Availability::Unavailable);
    assert_eq!(
        targets.availability(&repo(&s), &item(&path, registered, "engine_output")),
        Availability::Unavailable
    );
    assert_eq!(
        targets.availability(&repo(&s), &item(&path, registered, "observed_backend_plan")),
        Availability::Unavailable
    );
    assert_eq!(calls.get(), 0);
    assert_eq!(targets.command_failure(), None);
}

#[test]
fn a_working_lookup_is_read_once_for_a_whole_page() {
    let s = scratch("once");
    let (targets, calls) = counting(Ok("nobody-at-all".into()));
    for name in ["a", "b", "c"] {
        let path = file(&s, &format!("out/{name}.txt"));
        assert_eq!(state(&targets, &s, &path), Availability::Available);
    }
    assert_eq!(calls.get(), 1);
    assert_eq!(targets.command_failure(), None);
}

#[test]
fn an_account_entry_that_is_a_regular_file_refuses_each_artifact_but_not_the_command() {
    let s = scratch("regular");
    let entry = s.0.join("acct");
    let path = file(&s, "out/a.txt");
    let (empty, _) = counting(Ok(account_at(&s, "acct")));
    fs::create_dir(&entry).unwrap();
    assert_eq!(
        state(&empty, &s, &path),
        Availability::Available,
        "an empty directory protects nothing"
    );
    fs::remove_dir(&entry).unwrap();
    fs::write(&entry, b"not a directory").unwrap();
    let (targets, _) = counting(Ok(account_at(&s, "acct")));
    assert_eq!(state(&targets, &s, &path), Availability::Unavailable);
    assert_eq!(
        state(&targets, &s, &file(&s, "out/b.txt")),
        Availability::Unavailable
    );
    assert_eq!(
        targets.command_failure(),
        None,
        "the command goes on; only the artifacts fail"
    );
}

#[test]
fn an_account_without_a_directory_protects_nothing() {
    let s = scratch("absent");
    let path = file(&s, "out/a.txt");
    let (targets, _) = counting(Ok(account_at(&s, "never-created")));
    assert_eq!(state(&targets, &s, &path), Availability::Available);
}

#[test]
fn the_plist_of_the_running_account_protects_its_symlink_target_and_another_accounts_does_not() {
    let s = scratch("accounts");
    let policy = file(&s, "policy.json");
    for account in ["mine", "theirs"] {
        let directory = s.0.join(account);
        fs::create_dir(&directory).unwrap();
        symlink(&policy, directory.join(PLIST)).unwrap();
    }
    let (mine, _) = counting(Ok(account_at(&s, "mine")));
    assert_eq!(state(&mine, &s, &policy), Availability::Unavailable);
    let (nobody, _) = counting(Ok(account_at(&s, "nobody")));
    assert_eq!(
        state(&nobody, &s, &policy),
        Availability::Available,
        "another account's directory has no effect"
    );
    let ordinary = file(&s, "out/a.txt");
    assert_eq!(state(&mine, &s, &ordinary), Availability::Available);
}

#[test]
fn the_ambient_account_is_looked_up_for_real_and_the_computer_level_plist_is_not_listed() {
    let s = scratch("ambient");
    let path = file(&s, "out/a.txt");
    let targets = CommandArtifactTargets::new();
    assert_eq!(state(&targets, &s, &path), Availability::Available);
    assert_eq!(targets.command_failure(), None);
}
