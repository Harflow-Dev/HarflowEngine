use super::{Owned, deadline, id};
use hf_application::process::IdentityPort;
use hf_platform::process::{MacIdentity, WaitOutcome};
use std::{path::Path, path::PathBuf, time::Instant};

pub fn oracle_process(json: &str, key: &str) -> hf_domain::process::ProcessIdentity {
    let marker = format!("\"{key}\": \"");
    let encoded = json
        .split(&marker)
        .nth(1)
        .expect("oracle identity field")
        .split('"')
        .next()
        .expect("identity terminator");
    let mut fields = encoded.split(':');
    hf_domain::process::ProcessIdentity::new(
        fields.next().expect("PID").parse().expect("PID integer"),
        fields
            .next()
            .expect("start seconds")
            .parse()
            .expect("seconds integer"),
        fields
            .next()
            .expect("start micros")
            .parse()
            .expect("micros integer"),
        fields.next().expect("boot UUID").to_owned(),
    )
    .expect("valid process identity")
}

pub fn oracle_generation(json: &str) -> hf_domain::ownership::Id {
    let value = json
        .split("\"generation\": \"")
        .nth(1)
        .expect("oracle generation field")
        .split('"')
        .next()
        .expect("oracle generation terminator");
    id(value)
}

pub fn effect_child_pid(owner_pid: u32, cwd: &Path) -> u32 {
    process_child_pid(owner_pid, "--internal-effect-fixture-v1", cwd)
}

pub fn run_python_fixture(name: &str, root: &Path, operation: &str, cwd: &Path) -> String {
    let script = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/handoff_losses/support")
        .join(name);
    let mut child = Owned::spawn(
        PathBuf::from("/usr/bin/python3"),
        vec![
            "-I".into(),
            "-S".into(),
            script.into_os_string(),
            root.as_os_str().into(),
            operation.into(),
        ],
        cwd.to_path_buf(),
    );
    child.0.close_stdin();
    let until = deadline(2);
    assert!(
        matches!(
            child.0.wait_until(until, || false),
            Ok(WaitOutcome::Exited(hf_domain::process::ProcessExit::Code(
                0
            )))
        ),
        "fixed Python fixture {name} failed"
    );
    drain_capture(&mut child, until).expect("bounded fixed-Python output drain");
    assert!(
        !child.0.capture().stdout.overflow,
        "fixed Python fixture output truncated"
    );
    String::from_utf8(child.0.capture().stdout.bytes.clone()).expect("fixed Python fixture output")
}

pub fn process_child_pid(parent_pid: u32, arg: &str, cwd: &Path) -> u32 {
    let script = "import subprocess; print(subprocess.check_output(['/bin/ps','-axo','pid=,ppid=,command='], text=True, timeout=1.0), end='')";
    let mut ps = Owned::spawn(
        PathBuf::from("/usr/bin/python3"),
        vec!["-I".into(), "-S".into(), "-c".into(), script.into()],
        cwd.to_path_buf(),
    );
    ps.0.close_stdin();
    let until = deadline(2);
    assert!(
        matches!(
            ps.0.wait_until(until, || false),
            Ok(WaitOutcome::Exited(hf_domain::process::ProcessExit::Code(
                0
            )))
        ),
        "bounded process inventory failed"
    );
    drain_capture(&mut ps, until).expect("bounded process-table output drain");
    assert!(
        !ps.0.capture().stdout.overflow,
        "process table snapshot must be complete"
    );
    let rows =
        String::from_utf8(ps.0.capture().stdout.bytes.clone()).expect("process inventory output");
    rows.lines()
        .find_map(|line| {
            let mut fields = line.split_whitespace();
            let pid = fields.next()?.parse::<u32>().ok()?;
            let ppid = fields.next()?.parse::<u32>().ok()?;
            let command = fields.collect::<Vec<_>>().join(" ");
            (ppid == parent_pid && command.contains(arg)).then_some(pid)
        })
        .expect("fixed direct child process identity")
}

pub fn wait_identity_gone(pid: u32) {
    let until = deadline(6);
    loop {
        match pid_presence(pid) {
            Presence::Absent => return,
            Presence::Present => {}
            Presence::Unknown(reason) => panic!("PID table observation failed: {reason}"),
        }
        assert!(
            Instant::now() < until,
            "cooperative fixture child remains in kernel process table"
        );
        std::thread::yield_now();
    }
}

pub fn identity_is_current(identity: &hf_domain::process::ProcessIdentity) -> bool {
    MacIdentity.identity(identity.pid()).as_ref() == Some(identity)
}

enum Presence {
    Present,
    Absent,
    Unknown(String),
}
fn pid_presence(pid: u32) -> Presence {
    let script = "import subprocess,sys;\ntry:\n p=subprocess.run(['/bin/ps','-p',sys.argv[1],'-o','pid='],text=True,capture_output=True,timeout=1.0)\n print(f'{p.returncode}|{p.stdout.strip()}|{p.stderr.strip()}')\nexcept subprocess.TimeoutExpired:\n print('TIMEOUT||')";
    let mut ps = Owned::spawn(
        PathBuf::from("/usr/bin/python3"),
        vec![
            "-I".into(),
            "-S".into(),
            "-c".into(),
            script.into(),
            pid.to_string().into(),
        ],
        PathBuf::from("/private/tmp"),
    );
    ps.0.close_stdin();
    let until = deadline(2);
    let outcome = ps.0.wait_until(until, || false);
    if !matches!(
        outcome,
        Ok(WaitOutcome::Exited(hf_domain::process::ProcessExit::Code(
            0
        )))
    ) {
        return Presence::Unknown(format!("process inventory outcome {outcome:?}"));
    }
    if let Err(error) = drain_capture(&mut ps, until) {
        return Presence::Unknown(error);
    }
    if !ps.0.capture().stdout.eof || !ps.0.capture().stderr.eof {
        return Presence::Unknown("process table output did not reach EOF".into());
    }
    if ps.0.capture().stdout.overflow || ps.0.capture().stderr.error.is_some() {
        return Presence::Unknown("truncated or errored process-table output".into());
    }
    let output = String::from_utf8_lossy(&ps.0.capture().stdout.bytes)
        .trim()
        .to_owned();
    match output.split_once('|') {
        Some(("0", fields)) if fields == format!("{pid}|") => Presence::Present,
        Some(("1", "|")) => Presence::Absent,
        _ => Presence::Unknown(output),
    }
}

pub fn drain_capture(child: &mut Owned, until: Instant) -> Result<(), String> {
    while !child.0.capture().stdout.eof || !child.0.capture().stderr.eof {
        if Instant::now() >= until {
            return Err("bounded capture drain expired".into());
        }
        child
            .0
            .pump(until, 65536)
            .map_err(|error| error.to_string())?;
        if let Some(error) = &child.0.capture().stdout.error {
            return Err(format!("stdout capture failed: {error}"));
        }
        if let Some(error) = &child.0.capture().stderr.error {
            return Err(format!("stderr capture failed: {error}"));
        }
    }
    Ok(())
}
