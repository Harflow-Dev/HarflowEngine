use super::fail_settled_child;
use super::*;

pub fn path_boundary_and_alias_counterexample() {
    let mut owned = SettledRoot::create();
    let root = &owned.fixture;
    let outside = root.outside().join("witness");
    fs::write(&outside, b"outside-original").unwrap();
    let (code, result) = launch(root, "write", &outside);
    assert_eq!(code, 0);
    assert_eq!(result.trim(), "RESULT denied errno=1");
    assert_eq!(fs::read(&outside).unwrap(), b"outside-original");

    let allowed = root.workspace().join("positive-control");
    let (code, result) = launch(root, "write", &allowed);
    assert_eq!(code, 0);
    assert_eq!(result.trim(), "RESULT allowed");
    assert_eq!(fs::read(&allowed).unwrap(), b"changed-by-alias");

    assert_direct_path_boundaries(root);

    let symlink_target = root.outside().join("symlink-target");
    fs::write(&symlink_target, b"symlink-original").unwrap();
    let symlink = root.workspace().join("escape");
    std::os::unix::fs::symlink(&symlink_target, &symlink).unwrap();
    let (code, result) =
        launch_with_policy(root, ProfilePolicy::AliasCounterexample, "write", &symlink);
    assert_eq!(code, 0);
    assert_eq!(
        result.trim(),
        "RESULT denied errno=1",
        "write through symlink to denied outside path"
    );
    assert_eq!(fs::read(&symlink_target).unwrap(), b"symlink-original");

    let alias = root.workspace().join("existing-hardlink");
    fs::hard_link(&outside, &alias).unwrap();
    let profile = root
        .profile(
            ProfilePolicy::AliasCounterexample,
            &qualification_reads(),
            None,
        )
        .unwrap();
    profile_compile_control(root, ProfilePolicy::AliasCounterexample, &profile);
    let spec = root
        .spawn_spec(
            &profile,
            Path::new(PYTHON),
            &[
                "-I".into(),
                "-S".into(),
                FIXTURE.into(),
                "write".into(),
                alias.as_os_str().to_owned(),
            ],
            Vec::new(),
            4096,
        )
        .unwrap();
    log_profile(
        "existing-hardlink",
        ProfilePolicy::AliasCounterexample,
        &profile,
    );
    let mut child = RetainedOwnedChild::spawn(spec, &MacIdentity, deadline(3000)).unwrap();
    if child.initialization() != SpawnInitialization::Ready {
        settle_or_abort(&mut child, WaitOutcome::DeadlineExpired);
    }
    let outcome = match child.wait_until(deadline(5000), || false) {
        Ok(outcome) => outcome,
        Err(_) => settle_or_abort(&mut child, WaitOutcome::DeadlineExpired),
    };
    match outcome {
        WaitOutcome::Exited(ProcessExit::Code(0)) => {}
        WaitOutcome::Exited(exit) => fail_settled_child(&mut child, exit),
        other => settle_or_abort(&mut child, other),
    }
    let metadata = fs::metadata(&outside).unwrap();
    assert_eq!(metadata.nlink(), 2, "alias witness must remain linked");
    assert_eq!(
        fs::read(&outside).unwrap(),
        b"changed-by-alias",
        "pre-existing hardlink is normative counterexample evidence"
    );
    super::aliases::new_hardlink_syscalls_are_denied();
    owned.settle();
}

fn assert_direct_path_boundaries(root: &FixtureRoot) {
    let protected = root.protected().join("witness");
    let (code, result) = launch(root, "write", &protected);
    assert_eq!(code, 0);
    assert_eq!(result.trim(), "RESULT denied errno=1");
    assert!(!protected.exists());
    for operation in ["append", "unlink", "rename", "chmod", "symlink"] {
        assert_outside_operation_denied(root, operation);
    }
}

fn assert_outside_operation_denied(root: &FixtureRoot, operation: &str) {
    let target = root.outside().join(format!("{operation}-witness"));
    if operation != "symlink" {
        fs::write(&target, b"preserve-me").unwrap();
    }
    let before_mode = target
        .exists()
        .then(|| fs::metadata(&target).unwrap().mode());
    let (code, result) = launch(root, operation, &target);
    assert_eq!(code, 0, "{operation} helper should report observation");
    assert_eq!(
        result.trim(),
        "RESULT denied errno=1",
        "outside {operation} must be denied"
    );
    assert_eq!(
        target.exists(),
        operation != "symlink",
        "target state after {operation}"
    );
    if let Some(mode) = before_mode {
        assert_eq!(
            fs::read(&target).unwrap(),
            b"preserve-me",
            "outside witness bytes after {operation}"
        );
        assert_eq!(
            fs::metadata(&target).unwrap().mode(),
            mode,
            "outside witness mode after {operation}"
        );
    }
    assert!(
        !target.with_extension("moved").exists(),
        "rename destination absent after denial"
    );
}

pub fn ancestor_rename_counterexample() {
    let mut owned = SettledRoot::create();
    let root = &owned.fixture;
    let protected = root.protected();
    let ancestor = protected.parent().unwrap().to_path_buf();
    let witness = protected.join("bytes");
    fs::write(&witness, b"ancestor-witness").unwrap();
    let (code, result) = launch_with_policy(
        root,
        ProfilePolicy::AliasCounterexample,
        "ancestor-escape",
        &ancestor,
    );
    assert_eq!(code, 0);
    assert_eq!(
        result.trim(),
        "RESULT allowed",
        "allowed ancestor rename and subsequent moved-path write should succeed in one child"
    );
    let moved_witness = ancestor.with_extension("moved").join("control/bytes");
    assert_eq!(fs::read(&moved_witness).unwrap(), b"changed-by-alias");
    super::aliases::hardened_ancestor_rename_is_denied();
    owned.settle();
}
