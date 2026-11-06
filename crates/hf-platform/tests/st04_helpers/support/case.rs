use super::{Build, file_identity, write_case_report, write_manifest};
use hf_domain::process::ProcessExit;
use hf_platform::{
    process::MacIdentity,
    qualification::FixtureRoot,
    st04_fixture::{HelperLaunch, ProbeControl},
};
use std::{
    fs,
    os::unix::fs::MetadataExt,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

pub(crate) struct CaseResult {
    pub(crate) identity: String,
    pub(crate) exit: String,
    pub(crate) stdout: Vec<u8>,
    pub(crate) stderr: Vec<u8>,
    pub(crate) stdout_eof: bool,
    pub(crate) stderr_eof: bool,
    pub(crate) stdout_overflow: bool,
    pub(crate) stderr_overflow: bool,
    pub(crate) probe_absent: bool,
    pub(crate) probe_bytes: Vec<u8>,
    pub(crate) probe_device: Option<u64>,
    pub(crate) probe_inode: Option<u64>,
    pub(crate) probe_len: u64,
    pub(crate) network_bytes: Vec<u8>,
    pub(crate) settled: bool,
    pub(crate) absence_qualified: bool,
    pub(crate) assertions: Vec<String>,
}

pub(crate) fn run_case(control: ProbeControl) -> (PathBuf, CaseResult) {
    execute_case(prepare_case(control), control)
}

struct PreparedCase {
    directory: PathBuf,
    root: PathBuf,
    launch: HelperLaunch,
    manifest_hash: String,
}

fn prepare_case(control: ProbeControl) -> PreparedCase {
    let directory = super::evidence_dir();
    let fixture = FixtureRoot::create().expect("fresh isolated physical fixture");
    let root = fixture.root().to_path_buf();
    let (root_dev, root_ino, _) = file_identity(&root);
    super::write_synced_bytes(
        &directory.join("root-before-issuance.txt"),
        format!("{} {root_dev} {root_ino}\n", root.display()).as_bytes(),
    );
    let build = Build::compile(&root, &directory);
    let mut launch = if control == ProbeControl::None {
        HelperLaunch::prepare(fixture, build.binary.clone())
    } else {
        HelperLaunch::prepare_control(fixture, build.binary.clone(), control)
    }
    .expect("prepare fixed helper assignment");
    let controller = launch
        .enable_controller_signal_probe()
        .expect("enroll dedicated test controller");
    let spec = launch
        .spawn_spec()
        .expect("capture exact prepared SpawnSpec");
    assert_eq!(spec.executable, Path::new("/usr/bin/sandbox-exec"));
    assert!(
        spec.environment.is_empty(),
        "helper environment is explicitly empty"
    );
    assert_eq!(spec.capture_limit, 4096);
    let argv = spec
        .args
        .iter()
        .map(|value| value.to_str().expect("UTF-8 fixed argv").to_owned())
        .collect::<Vec<_>>();
    let manifest_hash = write_manifest(
        &directory,
        super::Manifest {
            control,
            root: &root,
            build: &build,
            profile: launch.profile(),
            args: &argv,
            cwd: &spec.cwd,
            controller: &format!("{controller:?}"),
        },
    );
    PreparedCase {
        directory,
        root,
        launch,
        manifest_hash,
    }
}

fn execute_case(prepared: PreparedCase, control: ProbeControl) -> (PathBuf, CaseResult) {
    let PreparedCase {
        directory,
        root,
        launch,
        manifest_hash,
    } = prepared;
    let mut measurement = launch
        .launch(&MacIdentity, Instant::now() + Duration::from_secs(8))
        .expect("launch exact enrolled helper through fixed sandbox seam");
    if let Err(error) = measurement.release(Instant::now() + Duration::from_secs(5)) {
        let result = from_partial(&mut measurement, error.to_string());
        write_case_report(&directory, &manifest_hash, &result);
        std::mem::forget(measurement);
        panic!(
            "release refused or uncertain; retained root {}",
            root.display()
        );
    }
    let diagnostic = match measurement.settle(Instant::now() + Duration::from_secs(12)) {
        Ok(value) => value,
        Err(error) => {
            let result = from_partial(&mut measurement, error.to_string());
            write_case_report(&directory, &manifest_hash, &result);
            std::mem::forget(measurement);
            panic!("settlement unknown; retained root {}", root.display());
        }
    };
    let probe = root.join("workspace/st04-effect");
    let (probe_bytes, probe_device, probe_inode, probe_len) = read_probe(&probe);
    let child_capture = measurement.child_mut().capture().clone();
    let result = CaseResult {
        identity: format!("{:?}", diagnostic.identity),
        exit: format!("{:?}", diagnostic.exit),
        stdout: child_capture.stdout.bytes,
        stderr: child_capture.stderr.bytes,
        stdout_eof: child_capture.stdout.eof,
        stderr_eof: child_capture.stderr.eof,
        stdout_overflow: child_capture.stdout.overflow,
        stderr_overflow: child_capture.stderr.overflow,
        probe_absent: diagnostic.probe_absent,
        probe_bytes,
        probe_device,
        probe_inode,
        probe_len,
        network_bytes: diagnostic.network_bytes.clone(),
        settled: diagnostic.settled,
        absence_qualified: diagnostic.absence_qualified,
        assertions: vec!["raw observation saved before assertions".into()],
    };
    write_case_report(&directory, &manifest_hash, &result);
    validate(control, &diagnostic, &result);
    let root = measurement.root().root().to_path_buf();
    measurement
        .root()
        .validate_topology()
        .expect("final bounded inventory");
    drop(measurement);
    fs::remove_dir_all(&root).expect("clean only after original helper reaped and evidence saved");
    println!("ST04 {control:?} evidence: {}", directory.display());
    (directory, result)
}

fn from_partial(
    measurement: &mut hf_platform::st04_fixture::HelperMeasurement,
    error: String,
) -> CaseResult {
    let capture = measurement.child_mut().capture().clone();
    let mut result = CaseResult {
        identity: format!("{:?}", measurement.child_mut().registered_identity()),
        exit: format!("{:?}; {error}", measurement.child_mut().exit()),
        stdout: capture.stdout.bytes,
        stderr: capture.stderr.bytes,
        stdout_eof: capture.stdout.eof,
        stderr_eof: capture.stderr.eof,
        stdout_overflow: capture.stdout.overflow,
        stderr_overflow: capture.stderr.overflow,
        probe_absent: false,
        probe_bytes: Vec::new(),
        probe_device: None,
        probe_inode: None,
        probe_len: 0,
        network_bytes: Vec::new(),
        settled: false,
        absence_qualified: false,
        assertions: Vec::new(),
    };
    result
        .assertions
        .push("unknown outcome; root retained".into());
    result
}

fn read_probe(path: &Path) -> (Vec<u8>, Option<u64>, Option<u64>, u64) {
    use std::io::Read;
    use std::os::unix::fs::OpenOptionsExt;
    let before = match fs::symlink_metadata(path) {
        Ok(value) => value,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return (Vec::new(), None, None, 0);
        }
        Err(error) => panic!("probe identity unknown; retain fixture: {error}"),
    };
    assert!(before.is_file() && !before.file_type().is_symlink() && before.nlink() == 1);
    assert!(before.len() <= 4096);
    let file = fs::OpenOptions::new()
        .read(true)
        .custom_flags(0x100) // Darwin O_NOFOLLOW
        .open(path)
        .expect("open bounded probe without following links");
    let mut bytes = Vec::new();
    file.take(4097)
        .read_to_end(&mut bytes)
        .expect("read bounded probe");
    let after = fs::symlink_metadata(path).expect("probe identity after read");
    assert_eq!(
        (before.dev(), before.ino(), before.len()),
        (after.dev(), after.ino(), after.len())
    );
    assert_eq!(bytes.len() as u64, after.len());
    (bytes, Some(after.dev()), Some(after.ino()), after.len())
}

fn validate(
    control: ProbeControl,
    diagnostic: &hf_platform::st04_fixture::Diagnostic,
    result: &CaseResult,
) {
    assert_eq!(diagnostic.exit, ProcessExit::Code(0));
    assert!(diagnostic.identity.is_some());
    assert!(result.stdout_eof && result.stderr_eof);
    assert!(!result.stdout_overflow && !result.stderr_overflow);
    assert_eq!(result.stderr, b"ST04-STDERR\n");
    let actual: Vec<_> = String::from_utf8(result.stdout.clone())
        .expect("UTF-8 helper protocol")
        .lines()
        .map(str::to_owned)
        .collect();
    let mut index = 0;
    expect_line(&actual, &mut index, "ST04-READY");
    expect_line(
        &actual,
        &mut index,
        marker("WRITE", control == ProbeControl::Write).as_str(),
    );
    expect_line(
        &actual,
        &mut index,
        marker("READ", control == ProbeControl::Read).as_str(),
    );
    expect_process_lines(&actual, &mut index, control == ProbeControl::Fork);
    expect_line(
        &actual,
        &mut index,
        marker("NETWORK", control == ProbeControl::Network).as_str(),
    );
    expect_line(
        &actual,
        &mut index,
        marker("SIGNAL", control == ProbeControl::Signal).as_str(),
    );
    assert_eq!(index, actual.len(), "no unclassified stdout bytes");
    assert_eq!(diagnostic.probe_absent, control != ProbeControl::Write);
    assert_eq!(
        result.probe_bytes,
        if control == ProbeControl::Write {
            b"ST04-EFFECT\n".to_vec()
        } else {
            Vec::new()
        }
    );
    assert_eq!(
        diagnostic.network_bytes,
        if control == ProbeControl::Network {
            b"ST04-NETWORK\n".to_vec()
        } else {
            Vec::new()
        }
    );
    assert_eq!(
        diagnostic.network_observed,
        control == ProbeControl::Network
    );
    assert_eq!(diagnostic.settled, control == ProbeControl::None);
    assert!(!diagnostic.absence_qualified);
    if control == ProbeControl::Write {
        assert_eq!(result.probe_len, 12);
        assert!(result.probe_device.is_some() && result.probe_inode.is_some());
    }
}

fn marker(name: &str, allowed: bool) -> String {
    format!("ST04-{name}-{}", if allowed { "ALLOWED" } else { "DENIED" })
}

fn expect_process_lines(lines: &[String], index: &mut usize, allowed: bool) {
    for name in ["fork", "spawn"] {
        if allowed {
            let line = lines.get(*index).expect("controlled child wait line");
            let fields: Vec<_> = line.split_whitespace().collect();
            assert!(
                fields.len() == 5
                    && fields[0] == "ST04-CHILD"
                    && fields[1] == name
                    && fields[3] == "wait"
                    && fields[4] == "0"
            );
            let pid = fields[2]
                .parse::<u32>()
                .expect("reported positive-control child PID");
            assert_ne!(pid, 0);
            *index += 1;
        }
        let label = if name == "fork" { "FORK" } else { "SPAWN" };
        expect_line(lines, index, marker(label, allowed).as_str());
    }
}

fn expect_line(lines: &[String], index: &mut usize, expected: &str) {
    assert_eq!(lines.get(*index).map(String::as_str), Some(expected));
    *index += 1;
}
