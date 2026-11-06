#![allow(dead_code)]
#![cfg(all(target_os = "macos", target_arch = "aarch64"))]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use hf_application::{
    owner_runtime::DigestPort, prepared_launch::ReceiptDeliveryPort, recovery::RecoveryError,
};
use hf_domain::{
    capture::FileIdentity,
    fixture::{Frame, ParentPlan, QualifiedLedger},
    owner_policy::{
        AuthorityContext, AuthorityRole, FixtureAssignment, PermissionMode, PreparationEnvelope,
        Requirement, RuntimePolicy,
    },
    ownership::{Digest, Id, PreparedRequest},
    prepared_launch::{BinaryPin, LaunchAssignment, PreparedLaunch},
    process::ProcessIdentity,
    recovery::SourceStamp,
};
use hf_interfaces::{fixture, owner_preparation, prepared_launch};
use hf_platform::process::{MacIdentity, RetainedOwnedChild, SpawnSpec, WaitOutcome};
use hf_storage::Sha256Digest;
use std::{
    ffi::OsString,
    fs::{self, OpenOptions},
    os::unix::fs::MetadataExt,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

pub(super) fn deadline(seconds: u64) -> Instant {
    Instant::now() + Duration::from_secs(seconds)
}
pub(super) fn id(c: char) -> Id {
    Id::new(c.to_string().repeat(32)).unwrap()
}
pub(super) fn digest(c: char) -> Digest {
    Digest::new(c.to_string().repeat(64)).unwrap()
}
pub(super) fn fake_process(pid: u32) -> ProcessIdentity {
    ProcessIdentity::new(pid, 100, 7, "abcdef01-2345-6789-abcd-ef0123456789".into()).unwrap()
}
pub(super) fn fake_source() -> SourceStamp {
    let file = |inode| FileIdentity { device: 1, inode };
    SourceStamp {
        generation: id('5'),
        root: file(10),
        database: file(11),
        marker: file(12),
        store_lock: file(13),
        io_lock: file(14),
    }
}
pub(super) fn binary() -> PathBuf {
    std::env::current_exe()
        .unwrap()
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("harflow-engine-scaffold")
}
pub(super) fn scratch() -> PathBuf {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let path = PathBuf::from("/private/tmp").join(format!("hf-hfpl-{}-{n}", std::process::id()));
    fs::create_dir(&path).unwrap();
    fs::canonicalize(path).unwrap()
}
pub(super) struct CurrentDirectory(PathBuf);
impl CurrentDirectory {
    pub(super) fn enter(path: &PathBuf) -> Self {
        let previous = std::env::current_dir().unwrap();
        std::env::set_current_dir(path).unwrap();
        Self(previous)
    }
}
impl Drop for CurrentDirectory {
    fn drop(&mut self) {
        std::env::set_current_dir(&self.0).expect("restore test directory");
    }
}
pub(super) fn ledger(path: &Path) -> QualifiedLedger {
    let file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path.join("r03-b2.effects"))
        .unwrap();
    QualifiedLedger {
        device: file.metadata().unwrap().dev(),
        inode: file.metadata().unwrap().ino(),
    }
}
pub(super) fn request(
    caller: hf_domain::process::ProcessIdentity,
    cwd: &str,
    pin: BinaryPin,
    ledger: QualifiedLedger,
    source: hf_domain::recovery::SourceStamp,
) -> PreparedRequest {
    let mut policy = PreparationEnvelope {
        profile_digest: Digest::new(owner_preparation::PROFILE_DIGEST.into()).unwrap(),
        configuration_digest: digest('a'),
        operation: id('1'),
        session: id('2'),
        reservation: id('3'),
        attempt: id('4'),
        generation: source.generation.clone(),
        policy: RuntimePolicy {
            background_requirement: Requirement::Required,
            permission_mode: PermissionMode::OsEnforced,
            background_idle_seconds: 600.0,
            tool_level_opt_in: false,
        },
        authority: AuthorityContext {
            caller,
            role: AuthorityRole::Direct,
            checkpoint: None,
            revision: None,
            issued_attempt: id('4'),
            grant_digest: digest('b'),
        },
        assignment: FixtureAssignment::FixedEffect,
    };
    let mut launch = PreparedLaunch {
        policy: policy.clone(),
        assignment: LaunchAssignment::FixedProtocolHelper,
        binary: pin,
        argv: vec!["--fixture-fixed-protocol-helper".into()],
        environment: Vec::new(),
        cwd: cwd.into(),
        source,
        ledger,
        source_profile: "fixed-protocol-helper-v1".into(),
    };
    policy.configuration_digest =
        prepared_launch::configuration_digest(&launch, &Sha256Digest).unwrap();
    launch.policy = policy;
    let payload = prepared_launch::encode(&launch, &Sha256Digest).unwrap();
    PreparedRequest {
        operation: id('1'),
        session: id('2'),
        reservation: id('3'),
        attempt: id('4'),
        generation: launch.policy.generation,
        digest: Sha256Digest.digest_parts(&[&payload]),
        payload,
    }
}
pub(super) fn stale_configuration(mut value: PreparedRequest) -> PreparedRequest {
    let policy_len = u32::from_be_bytes(value.payload[5..9].try_into().unwrap()) as usize;
    let mut at = 9 + policy_len + 1;
    for _ in 0..2 {
        let size = u32::from_be_bytes(value.payload[at..at + 4].try_into().unwrap()) as usize;
        at += 4 + size;
    }
    at += 16 + 40;
    value.payload[at] = if value.payload[at] == b'c' {
        b'd'
    } else {
        b'c'
    };
    value.digest = Sha256Digest.digest_parts(&[&value.payload]);
    value
}
pub(super) struct Delivery(pub(super) Option<hf_domain::prepared_launch::CaptureAccepted>);
impl ReceiptDeliveryPort for Delivery {
    fn receive(
        &self,
        invitation: &hf_domain::fixture::Invitation,
        _deadline: Instant,
    ) -> Result<Option<hf_domain::prepared_launch::CaptureAccepted>, RecoveryError> {
        Ok(self
            .0
            .as_ref()
            .filter(|r| &r.invitation == invitation)
            .cloned())
    }
}
pub(super) fn spawn_parent(cwd: &Path) -> RetainedOwnedChild {
    RetainedOwnedChild::spawn(
        SpawnSpec {
            executable: binary(),
            cwd: cwd.to_path_buf(),
            args: vec!["--internal-parent-fixture-v1".into()],
            environment: Vec::<(OsString, OsString)>::new(),
            capture_limit: 32768,
        },
        &MacIdentity,
        deadline(3),
    )
    .unwrap()
}
pub(super) fn send_plan(child: &mut RetainedOwnedChild, plan: ParentPlan) {
    child
        .write_fixture_frame(&fixture::encode(&Frame::Parent(plan)).unwrap(), deadline(3))
        .unwrap();
    child.close_stdin();
}
pub(super) fn receive(child: &mut RetainedOwnedChild) -> Frame {
    let bytes = child.read_fixture_frame(deadline(8)).unwrap();
    fixture::decode(&bytes).unwrap()
}
pub(super) fn finish(child: &mut RetainedOwnedChild) {
    assert!(matches!(
        child.wait_until(deadline(8), || false).unwrap(),
        WaitOutcome::Exited(_)
    ));
}
pub(super) fn run_parent(
    cwd: &Path,
    plan: impl FnOnce(ProcessIdentity) -> ParentPlan,
) -> (ProcessIdentity, Frame) {
    let mut child = spawn_parent(cwd);
    assert_eq!(
        child.initialization(),
        hf_platform::process::SpawnInitialization::Ready
    );
    let caller = child
        .registered_identity()
        .expect("retained caller identity")
        .clone();
    send_plan(&mut child, plan(caller.clone()));
    let frame = receive(&mut child);
    finish(&mut child);
    (caller, frame)
}
