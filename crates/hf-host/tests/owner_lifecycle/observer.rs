use super::support::{
    assert_identity_consistency, base, deadline, file_state, inspect, stable_projection,
    store_namespace,
};
use hf_application::process::IdentityPort;
use hf_application::recovery::RecoveryInspector;
use hf_domain::ownership::Id;
use hf_platform::{
    process::{MacIdentity, RetainedOwnedChild, SpawnSpec, WaitOutcome},
    recovery::{MacRecoveryIdentity, RawCaptureObserver},
};
use hf_storage::{Limits, RecoveryReadSession};
use std::{
    ffi::OsString,
    fs,
    io::{self, Read, Write},
    os::unix::net::{UnixListener, UnixStream},
    path::PathBuf,
    time::Instant,
};

#[test]
#[ignore = "fixed subprocess used only by owner_lifecycle integration tests"]
fn owner_lifecycle_readonly_observer_helper() -> Result<(), Box<dyn std::error::Error>> {
    let root = PathBuf::from(std::env::var_os("HARFLOW_R04D_ROOT").expect("observer root"));
    let scratch =
        PathBuf::from(std::env::var_os("HARFLOW_R04D_SCRATCH").expect("observer scratch"));
    let socket = PathBuf::from(std::env::var_os("HARFLOW_R04D_SOCKET").expect("observer socket"));
    let store = RecoveryReadSession::new(
        &root,
        &scratch,
        Limits::default(),
        hf_platform::lease::shared(),
    );
    let inspector =
        RecoveryInspector::new(store, MacRecoveryIdentity, RawCaptureObserver::new(&root));
    let report = inspector
        .inspect(&Id::new(base::OP.to_owned()).unwrap(), deadline(2))
        .unwrap()
        .unwrap();
    assert_identity_consistency(&report, true);
    let bytes = stable_projection(&report).into_bytes();
    assert!(bytes.len() <= 4096, "bounded observer output");
    let mut stream = UnixStream::connect(socket).expect("fixed observer gate");
    stream.set_nonblocking(true).unwrap();
    write_bounded(
        &mut stream,
        &(bytes.len() as u32).to_be_bytes(),
        deadline(1),
    )?;
    write_bounded(&mut stream, &bytes, deadline(1))?;
    let mut release = [0; 1];
    read_bounded(&mut stream, &mut release, deadline(8))?;
    Ok(())
}

pub fn start(fixture: &base::Fixture, number: u8) -> (base::Owned, UnixStream, Vec<u8>) {
    let path = fixture.scratch.join(format!("observer-{number}.sock"));
    let listener = UnixListener::bind(&path).expect("bind fixed observer gate");
    listener
        .set_nonblocking(true)
        .expect("nonblocking observer gate");
    let child = RetainedOwnedChild::spawn(
        SpawnSpec {
            executable: fs::canonicalize(std::env::current_exe().unwrap()).unwrap(),
            args: vec![
                "--ignored".into(),
                "--exact".into(),
                "observer::owner_lifecycle_readonly_observer_helper".into(),
                "--nocapture".into(),
            ],
            cwd: fs::canonicalize(std::env::current_dir().unwrap()).unwrap(),
            environment: vec![
                (
                    OsString::from("HARFLOW_R04D_ROOT"),
                    fixture.root.as_os_str().to_owned(),
                ),
                (
                    OsString::from("HARFLOW_R04D_SCRATCH"),
                    fixture.scratch.as_os_str().to_owned(),
                ),
                (
                    OsString::from("HARFLOW_R04D_SOCKET"),
                    path.as_os_str().to_owned(),
                ),
            ],
            capture_limit: 4096,
        },
        &MacIdentity,
        deadline(2),
    )
    .expect("spawn fixed read-only observer");
    let mut child = base::Owned(child);
    child.0.close_stdin();
    let mut peer = accept_bounded(&listener, deadline(3)).expect("accept observer connection");
    peer.set_nonblocking(true).unwrap();
    let mut header = [0; 4];
    read_bounded(&mut peer, &mut header, deadline(1)).expect("read report length");
    let length = u32::from_be_bytes(header) as usize;
    assert!(length <= 4096);
    let mut bytes = vec![0; length];
    read_bounded(&mut peer, &mut bytes, deadline(1)).expect("read report body");
    (child, peer, bytes)
}

pub fn prove_readonly_restart(
    fixture: &base::Fixture,
    original: &hf_domain::recovery::RecoveryReport,
) {
    let owner = original
        .record
        .ownership
        .owner
        .as_ref()
        .unwrap()
        .process
        .clone();
    let files = file_state(fixture);
    let namespace = store_namespace(fixture);
    let (mut first, first_peer, first_bytes) = start(fixture, 31);
    assert_eq!(first_bytes, stable_projection(original).into_bytes());
    assert_eq!(file_state(fixture), files);
    assert_eq!(store_namespace(fixture), namespace);
    base::kill_direct(&mut first);
    drop(first_peer);
    assert_eq!(MacIdentity.identity(owner.pid()), Some(owner.clone()));

    let (mut second, mut peer, second_bytes) = start(fixture, 32);
    assert_eq!(second_bytes, first_bytes);
    assert_eq!(file_state(fixture), files);
    assert_eq!(store_namespace(fixture), namespace);
    peer.write_all(&[1]).expect("release fixed observer gate");
    peer.shutdown(std::net::Shutdown::Write)
        .expect("close observer gate");
    finish(&mut second);
    let reread = inspect(fixture);
    assert_identity_consistency(&reread, true);
    assert_eq!(stable_projection(&reread), stable_projection(original));
    assert_eq!(
        fs::read_to_string(&fixture.effects)
            .unwrap()
            .lines()
            .count(),
        1
    );
}

pub fn finish(child: &mut base::Owned) {
    assert!(matches!(
        child.0.wait_until(deadline(2), || false),
        Ok(WaitOutcome::Exited(hf_domain::process::ProcessExit::Code(
            0
        )))
    ));
    base::drain_capture(child, deadline(1)).expect("bounded observer helper output");
    assert!(child.0.capture().stdout.eof && child.0.capture().stderr.eof);
    assert!(!child.0.capture().stdout.overflow && !child.0.capture().stderr.overflow);
}

fn accept_bounded(listener: &UnixListener, until: Instant) -> io::Result<UnixStream> {
    loop {
        match listener.accept() {
            Ok((stream, _)) => return Ok(stream),
            Err(error) if error.kind() == io::ErrorKind::WouldBlock && Instant::now() < until => {
                std::thread::yield_now()
            }
            Err(error) => return Err(error),
        }
    }
}
fn read_bounded(stream: &mut UnixStream, bytes: &mut [u8], until: Instant) -> io::Result<()> {
    let mut offset = 0;
    while offset < bytes.len() {
        match stream.read(&mut bytes[offset..]) {
            Ok(0) => return Err(io::ErrorKind::UnexpectedEof.into()),
            Ok(count) => offset += count,
            Err(error) if error.kind() == io::ErrorKind::WouldBlock && Instant::now() < until => {
                std::thread::yield_now()
            }
            Err(error) => return Err(error),
        }
    }
    Ok(())
}
fn write_bounded(stream: &mut UnixStream, bytes: &[u8], until: Instant) -> io::Result<()> {
    let mut offset = 0;
    while offset < bytes.len() {
        match stream.write(&bytes[offset..]) {
            Ok(0) => return Err(io::ErrorKind::WriteZero.into()),
            Ok(count) => offset += count,
            Err(error) if error.kind() == io::ErrorKind::WouldBlock && Instant::now() < until => {
                std::thread::yield_now()
            }
            Err(error) => return Err(error),
        }
    }
    Ok(())
}
