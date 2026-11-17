use super::*;

pub(in crate::fixture::capture::tests) fn primed_child(
    witness: &Path,
    control: &Path,
) -> RetainedOwnedChild {
    fs::create_dir(control).unwrap();
    spawn(witness, Some(control))
}

pub(super) fn spawn(witness: &Path, control: Option<&Path>) -> RetainedOwnedChild {
    let mut args = vec![
        OsString::from("-I"),
        OsString::from("-S"),
        OsString::from(HELPER),
        witness.as_os_str().to_owned(),
    ];
    if let Some(control) = control {
        args.push(control.as_os_str().to_owned());
    }
    RetainedOwnedChild::spawn(
        SpawnSpec {
            executable: PathBuf::from("/usr/bin/python3"),
            args,
            cwd: fs::canonicalize(std::env::current_dir().unwrap()).unwrap(),
            environment: Vec::new(),
            capture_limit: 1,
        },
        &MacIdentity,
        Instant::now() + Duration::from_secs(3),
    )
    .unwrap()
}

pub(in crate::fixture::capture::tests) fn wait_marker(path: &Path, until: Instant) {
    loop {
        assert!(Instant::now() < until, "producer marker deadline: {path:?}");
        if path.is_file() {
            return;
        }
        std::thread::sleep(Duration::from_millis(1));
    }
}

pub(in crate::fixture::capture::tests) fn release_marker(control: &Path, name: &str) {
    fs::write(control.join(name), b"release").unwrap();
}

pub(in crate::fixture::capture::tests) fn wait_enqueued(
    view: &hf_platform::process::ReaderView,
    expected: (u64, u64),
    until: Instant,
) {
    loop {
        assert!(
            Instant::now() < until,
            "reader enqueue deadline: {expected:?}"
        );
        let progress = view.snapshot();
        let enqueued = (progress.stdout.enqueued, progress.stderr.enqueued);
        if enqueued == expected {
            return;
        }
        assert!(
            enqueued.0 <= expected.0 && enqueued.1 <= expected.1,
            "reader passed priming boundary: {enqueued:?} > {expected:?}"
        );
        std::thread::sleep(Duration::from_millis(1));
    }
}
