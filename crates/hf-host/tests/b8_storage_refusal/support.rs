use super::{base, preparation};
use hf_application::lease::{LeaseError, LeaseMode};
use hf_domain::{
    fixture::{Frame, Gate},
    process::ProcessExit,
};
use hf_interfaces::fixture::encode;
use hf_platform::process::{RetainedOwnedChild, WaitOutcome};
use std::{
    fs,
    path::{Path, PathBuf},
    sync::{Mutex, MutexGuard},
};

static CASE_LOCK: Mutex<()> = Mutex::new(());
pub fn case_guard() -> MutexGuard<'static, ()> {
    CASE_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

pub struct Directory(PathBuf);
impl Directory {
    pub fn enter(path: &Path) -> Self {
        let previous = std::env::current_dir().unwrap();
        std::env::set_current_dir(path).unwrap();
        Self(previous)
    }
}
impl Drop for Directory {
    fn drop(&mut self) {
        std::env::set_current_dir(&self.0).unwrap();
    }
}

#[derive(Clone, Copy)]
pub enum Fault {
    Fence,
    CommitLost,
}
impl Fault {
    pub fn inject(self, scratch: &preparation::Scratch) {
        match self {
            Self::Fence => fs::write(
                scratch.root.join("store.fence"),
                format!(
                    "harflow-fence-1\n{}\nfailed\n",
                    scratch.request.generation.as_str()
                ),
            )
            .unwrap(),
            Self::CommitLost => {
                let generation = scratch.request.generation.as_str();
                let mut bytes = hf_application::watermark::initial_file(generation).unwrap();
                let payload =
                    hf_application::watermark::encode_payload(generation, u64::MAX / 2).unwrap();
                bytes[..payload.len()].copy_from_slice(&payload);
                fs::write(scratch.root.join("store.watermark"), bytes).unwrap();
            }
        }
    }
}

pub fn start_owner(scratch: &preparation::Scratch, gate: Gate) -> base::Owned {
    let mut owner = base::Owned::spawn(
        base::binary(),
        vec!["--internal-isolated-owner-fixture-v1".into()],
        scratch.path.clone(),
    );
    let Frame::Hello(identity) = base::response(&owner.0) else {
        panic!("owner hello required")
    };
    let prepared = scratch.prepared(identity, gate);
    send(&owner.0, &Frame::Invitation(prepared.invitation.clone()));
    assert_eq!(
        base::response(&owner.0),
        Frame::Ready(prepared.invitation.clone())
    );
    send(&owner.0, &Frame::Prepared(prepared));
    owner.0.close_stdin();
    owner
}
fn send(child: &RetainedOwnedChild, frame: &Frame) {
    child
        .write_fixture_frame(&encode(frame).unwrap(), base::deadline(2))
        .unwrap();
}

pub fn snapshot(scratch: &preparation::Scratch) -> String {
    let script =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/b8_storage_refusal/oracle.py");
    let mut oracle = base::Owned::spawn(
        PathBuf::from("/usr/bin/python3"),
        vec![
            "-I".into(),
            "-S".into(),
            script.into_os_string(),
            scratch.root.as_os_str().into(),
            preparation::OP.into(),
        ],
        scratch.path.clone(),
    );
    oracle.0.close_stdin();
    assert!(matches!(
        oracle.0.wait_until(base::deadline(2), || false),
        Ok(WaitOutcome::Exited(ProcessExit::Code(0)))
    ));
    base::drain_capture(&mut oracle, base::deadline(2)).unwrap();
    assert!(oracle.0.capture().stdout.eof && oracle.0.capture().stderr.eof);
    assert!(!oracle.0.capture().stdout.overflow && !oracle.0.capture().stderr.overflow);
    String::from_utf8(oracle.0.capture().stdout.bytes.clone()).unwrap()
}

pub fn settle_refused_owner(owner: &mut base::Owned) {
    owner.0.fixture_eof(base::deadline(10)).unwrap();
    assert!(matches!(
        owner.0.wait_until(base::deadline(2), || false),
        Ok(WaitOutcome::Exited(ProcessExit::Code(2)))
    ));
    base::drain_capture(owner, base::deadline(2)).unwrap();
    assert!(owner.0.capture().stdout.eof && owner.0.capture().stderr.eof);
}

pub fn assert_lease_busy(root: &Path) {
    let lease = hf_platform::lease::shared().acquire(
        &root.join("store.lock"),
        LeaseMode::Exclusive,
        false,
        base::deadline(1),
    );
    assert!(matches!(lease, Err(LeaseError::Busy)));
}
pub fn assert_lease_available(root: &Path) {
    let lease = hf_platform::lease::shared().acquire(
        &root.join("store.lock"),
        LeaseMode::Exclusive,
        false,
        base::deadline(1),
    );
    assert!(
        lease.is_ok(),
        "all original owner/writer holds released after settlement"
    );
}
