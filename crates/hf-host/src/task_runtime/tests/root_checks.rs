//! The two root checks of the dispatcher: an existing root with group or other access is
//! refused before the store is opened, and a root that changes before the launch intent is
//! refused before anything is recorded.
use super::{
    harness::{Ctx, deadline, dispatcher_hooks, open_reader, request, require_clean_environment},
    serial_case,
};
use crate::task_runtime::{run_for_ctl_exit, seams::DispatchPoint, store_options};
use hf_application::ctl::task_run::RunFailure;
use hf_domain::ctl::state_root::StateRoot;
use hf_storage::{Limits, Store};
use std::{
    cell::Cell,
    os::unix::fs::{MetadataExt, PermissionsExt},
    path::Path,
    rc::Rc,
};

fn mode(path: &Path) -> u32 {
    std::fs::metadata(path).unwrap().mode() & 0o777
}
fn chmod(path: &Path, mode: u32) {
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode)).unwrap();
}
/// Every file below `root` with its bytes and mode, sorted.
fn tree(root: &Path) -> Vec<(String, u32, Vec<u8>)> {
    let mut found = Vec::new();
    let mut pending = vec![root.to_path_buf()];
    while let Some(dir) = pending.pop() {
        for entry in std::fs::read_dir(&dir).unwrap() {
            let path = entry.unwrap().path();
            let meta = std::fs::symlink_metadata(&path).unwrap();
            let text = path.strip_prefix(root).unwrap().display().to_string();
            if meta.is_dir() {
                pending.push(path.clone());
                found.push((text, meta.mode() & 0o777, Vec::new()));
            } else {
                found.push((
                    text,
                    meta.mode() & 0o777,
                    std::fs::read(&path).unwrap_or_default(),
                ));
            }
        }
    }
    found.sort();
    found
}
fn refused(failure: RunFailure) -> String {
    match failure {
        RunFailure::Refused(text) => text,
        other => panic!("expected a refusal, got {other:?}"),
    }
}
fn launch_candidates(ctx: &Ctx) -> usize {
    open_reader(&ctx.root())
        .runtime_mutation_candidates(
            &hf_interfaces::task_runtime::CanonicalLaunchValidator,
            64,
            deadline(),
        )
        .unwrap()
        .len()
}

#[test]
fn run_refuses_group_accessible_existing_root_before_any_change() {
    let _serial = serial_case();
    require_clean_environment();
    let ctx = Ctx::new("loose-root");
    drop(
        Store::initialize_runtime(
            ctx.root(),
            Limits::default(),
            deadline(),
            hf_platform::lease::shared(),
            store_options::suppressed(),
        )
        .unwrap(),
    );
    assert_eq!(mode(&ctx.root()), 0o700, "a new root is owner-only");
    chmod(&ctx.root(), 0o755);
    let before = tree(&ctx.root());
    let reached = Rc::new(Cell::new(0));
    let seen = reached.clone();
    let hook = Rc::new(move |_: DispatchPoint, _: &_| {
        seen.set(seen.get() + 1);
        Ok(())
    });
    let _hooks = dispatcher_hooks(&ctx, Some(hook));
    let root = StateRoot::from_normalized(ctx.root().to_str().unwrap().into());
    let text = refused(run_for_ctl_exit(&root, &request(&ctx)).unwrap_err());
    assert!(text.contains("allows group or other access"), "{text}");
    assert!(text.contains("No state was changed."), "{text}");
    assert_eq!(reached.get(), 0, "no dispatch point was reached");
    assert_eq!(
        tree(&ctx.root()),
        before,
        "no store open, no intent, no file touched"
    );
    assert_eq!(mode(&ctx.root()), 0o755, "the mode is left to the user");
    assert_eq!(ctx.fake.starts(), 0);
    assert_eq!(launch_candidates(&ctx), 0, "no launch intent");
}

#[test]
fn root_mode_change_between_checks_refuses_before_intent() {
    let _serial = serial_case();
    require_clean_environment();
    let ctx = Ctx::new("mode-race");
    let root_path = ctx.root();
    let hook = Rc::new(move |point: DispatchPoint, _: &_| {
        if point == DispatchPoint::BeforeIntentRootCheck {
            chmod(&root_path, 0o755);
        }
        Ok(())
    });
    let _hooks = dispatcher_hooks(&ctx, Some(hook));
    let root = StateRoot::from_normalized(ctx.root().to_str().unwrap().into());
    let text = refused(run_for_ctl_exit(&root, &request(&ctx)).unwrap_err());
    assert!(text.contains("allows group or other access;"), "{text}");
    assert!(text.contains("No launch intent was recorded."), "{text}");
    assert_eq!(launch_candidates(&ctx), 0, "no launch intent");
    assert_eq!(ctx.fake.starts(), 0, "no owner");
}

#[test]
fn a_root_created_by_run_is_owner_only() {
    let _serial = serial_case();
    require_clean_environment();
    let ctx = Ctx::new("created-root");
    let root_path = ctx.root();
    let seen = Rc::new(Cell::new(None));
    let record = seen.clone();
    let hook = Rc::new(move |point: DispatchPoint, _: &_| {
        if point == DispatchPoint::AfterT0 {
            record.set(Some(mode(&root_path)));
            return Err(crate::task_runtime::Error::Io(std::io::Error::other(
                "injected failure",
            )));
        }
        Ok(())
    });
    let _hooks = dispatcher_hooks(&ctx, Some(hook));
    let root = StateRoot::from_normalized(ctx.root().to_str().unwrap().into());
    assert!(run_for_ctl_exit(&root, &request(&ctx)).is_err());
    assert_eq!(seen.get(), Some(0o700));
}

#[test]
fn run_refuses_a_symlinked_root_with_the_canonical_path_message() {
    let _serial = serial_case();
    require_clean_environment();
    let ctx = Ctx::new("link-root");
    let real = ctx.dir.join("real");
    std::fs::create_dir(&real).unwrap();
    chmod(&real, 0o700);
    std::os::unix::fs::symlink(&real, ctx.root()).unwrap();
    let root = StateRoot::from_normalized(ctx.root().to_str().unwrap().into());
    let text = refused(run_for_ctl_exit(&root, &request(&ctx)).unwrap_err());
    assert!(text.contains("non-symlinked directory"), "{text}");
    assert!(!text.contains("allows group or other access"), "{text}");
    assert!(text.contains("No state was changed."), "{text}");
    assert_eq!(std::fs::read_dir(&real).unwrap().count(), 0);
}
