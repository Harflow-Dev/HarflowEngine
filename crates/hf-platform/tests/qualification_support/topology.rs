use super::*;

pub fn topology_guard_refuses_aliases() {
    let mut owned = SettledRoot::create();
    let root = &owned.fixture;
    let external = root.outside().join("source");
    fs::write(&external, b"witness").unwrap();
    let alias = root.workspace().join("alias");
    fs::hard_link(external, &alias).unwrap();
    assert!(
        root.validate_topology().is_err(),
        "pre-existing hardlinks must refuse topology"
    );
    fs::remove_file(&alias).unwrap();
    let symlink = root.workspace().join("symlink");
    std::os::unix::fs::symlink(root.outside(), &symlink).unwrap();
    assert!(
        root.validate_topology().is_err(),
        "symlinks must refuse topology"
    );
    fs::remove_file(symlink).unwrap();
    let fifo = root.workspace().join("special-fifo");
    nix::unistd::mkfifo(
        &fifo,
        nix::sys::stat::Mode::S_IRUSR | nix::sys::stat::Mode::S_IWUSR,
    )
    .unwrap();
    assert!(
        root.validate_topology().is_err(),
        "special files must refuse topology"
    );
    fs::remove_file(fifo).unwrap();
    for index in 0..1025 {
        fs::create_dir(root.workspace().join(format!("entry-{index}"))).unwrap();
    }
    assert!(
        root.validate_topology().is_err(),
        "inventory overflow must refuse topology"
    );
    owned.settle();
}

pub fn topology_guard_refuses_replaced_roles() {
    for role in [
        "workspace",
        "runtime",
        "nested",
        "control",
        "outside",
        "git",
    ] {
        assert_role_replacement_refused(role);
    }
}

fn assert_role_replacement_refused(role: &str) {
    let mut owned = SettledRoot::create();
    let root = &owned.fixture;
    let original = match role {
        "workspace" => root.workspace(),
        "runtime" => root.runtime(),
        "nested" => root.workspace().join("nested"),
        "control" => root.protected(),
        "outside" => root.outside(),
        "git" => root.workspace().join(".git"),
        _ => unreachable!(),
    };
    let moved = root.root().join(format!("saved-{role}"));
    fs::rename(&original, &moved).unwrap();
    fs::create_dir(&original).unwrap();
    fs::set_permissions(&original, fs::Permissions::from_mode(0o700)).unwrap();
    assert!(
        root.validate_topology().is_err(),
        "replaced {role} role directory must fail identity validation"
    );
    fs::remove_dir(&original).unwrap();
    fs::rename(&moved, &original).unwrap();
    owned.settle();
}

pub fn safe_spawn_closes_unlisted_descriptors() {
    let mut owned = SettledRoot::create();
    let root = &owned.fixture;
    let secret = root.outside().join("fd-witness");
    let mut file = OpenOptions::new()
        .create_new(true)
        .read(true)
        .write(true)
        .open(&secret)
        .unwrap();
    file.write_all(b"before").unwrap();
    file.sync_all().unwrap();
    let descriptor = file.as_raw_fd();
    fcntl(&file, FcntlArg::F_SETFD(FdFlag::empty())).unwrap();
    let profile = root
        .profile(
            ProfilePolicy::ControlledHelper,
            &qualification_reads(),
            None,
        )
        .unwrap();
    profile_compile_control(root, ProfilePolicy::ControlledHelper, &profile);
    let descriptor_text = descriptor.to_string();
    log_profile(
        "inherited-fd-positive-control",
        ProfilePolicy::ControlledHelper,
        &profile,
    );
    run_preopened_descriptor_positive_control(root, &profile, &descriptor_text, &secret);
    file.set_len(0).unwrap();
    file.seek(SeekFrom::Start(0)).unwrap();
    file.write_all(b"before").unwrap();
    file.sync_all().unwrap();
    assert_safe_spawn_descriptor_closed(root, &profile, &descriptor_text);
    assert_eq!(
        fs::read(&secret).unwrap(),
        b"before",
        "unlisted descriptor must not survive safe spawn"
    );
    owned.settle();
}

fn run_preopened_descriptor_positive_control(
    root: &FixtureRoot,
    profile: &str,
    descriptor_text: &str,
    secret: &Path,
) {
    let positive = Command::new(PYTHON)
        .arg("-I")
        .arg("-S")
        .arg(FIXTURE)
        .arg("sandbox-parent")
        .arg(profile)
        .arg(root.workspace())
        .arg(descriptor_text)
        .env_clear()
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut positive = DirectChild::new(positive);
    let output = positive.wait_output(5000);
    assert!(
        output.status.success(),
        "pre-opened descriptor helper failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8(output.stdout).unwrap().trim(),
        "RESULT allowed",
        "pre-opened writable descriptor must bypass pathname denial"
    );
    assert!(
        fs::read(secret).unwrap().starts_with(b"changed"),
        "pre-opened descriptor positive control must modify the denied outside file"
    );
}

fn assert_safe_spawn_descriptor_closed(root: &FixtureRoot, profile: &str, descriptor_text: &str) {
    log_profile(
        "safe-spawn-fd-exclusion",
        ProfilePolicy::ControlledHelper,
        profile,
    );
    let spec = root
        .spawn_spec(
            profile,
            Path::new(PYTHON),
            &[
                "-I".into(),
                "-S".into(),
                FIXTURE.into(),
                "write-fd".into(),
                descriptor_text.into(),
            ],
            Vec::new(),
            4096,
        )
        .unwrap();
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
        other => settle_or_abort(&mut child, other),
    }
    let end = deadline(1000);
    while (!child.capture().stdout.eof || !child.capture().stderr.eof) && Instant::now() < end {
        child.pump(end, 8192).unwrap();
    }
    assert!(
        child.capture().stdout.eof && child.capture().stderr.eof,
        "settled helper streams must reach EOF within bound"
    );
    let (stdout, _) = child.take_capture_bytes();
    assert_eq!(
        String::from_utf8(stdout).unwrap().trim(),
        "RESULT error errno=9",
        "safe spawn must close the actual pre-opened descriptor with EBADF"
    );
}
