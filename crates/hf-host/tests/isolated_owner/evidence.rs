use crate::support::{self, Scratch, deadline};
use hf_application::{
    lease::{LeaseError, LeaseMode},
    recovery::RecoveryIdentityPort,
};
use hf_domain::{ownership::Phase, process::ProcessIdentity, recovery::ProcessState};
use hf_platform::recovery::MacRecoveryIdentity;
use hf_storage::{Limits, Store};
use std::{fs, os::unix::fs::MetadataExt, thread, time::Instant};

pub(super) fn wait_arrived(peer: &support::GatePeer, until: Instant) {
    while !peer.arrived() {
        assert!(Instant::now() < until, "writer did not reach finite gate");
        thread::yield_now();
    }
}

pub(super) fn assert_root_busy(root: &std::path::Path, until: Instant) {
    let result = hf_platform::lease::shared().acquire(
        &root.join("store.lock"),
        LeaseMode::Exclusive,
        false,
        until,
    );
    assert!(
        matches!(result, Err(LeaseError::Busy)),
        "writer must retain exclusive root lease"
    );
}

pub(super) fn wait_root_available(root: &std::path::Path, until: Instant) {
    if wait_root_available_result(root, until).is_err() {
        panic!("writer did not naturally release root lease before deadline");
    }
}

pub(super) fn wait_root_available_result(root: &std::path::Path, until: Instant) -> Result<(), ()> {
    loop {
        match hf_platform::lease::shared().acquire(
            &root.join("store.lock"),
            LeaseMode::Exclusive,
            false,
            until,
        ) {
            Ok(lease) => {
                drop(lease);
                return Ok(());
            }
            Err(LeaseError::Busy) if Instant::now() < until => thread::yield_now(),
            Err(LeaseError::Busy) => return Err(()),
            Err(_) => return Err(()),
        }
    }
}

pub(super) fn observe_agent(scratch: &Scratch) -> ProcessIdentity {
    let store = Store::open(
        &scratch.root,
        Limits::default(),
        deadline(2),
        hf_platform::lease::shared(),
    )
    .expect("open bounded fixture store observation");
    let snapshot = store
        .ownership_observe(&support::id(support::OP), deadline(2))
        .expect("observe fixed owner snapshot")
        .expect("prepared owner snapshot");
    assert!(matches!(
        snapshot.phase,
        Phase::SpawnObserved | Phase::AcceptanceRecorded
    ));
    let agent = snapshot
        .child
        .expect("persisted original fixed effect identity");
    drop(store);
    agent
}

pub(super) fn read_writer_identity(path: &std::path::Path) -> ProcessIdentity {
    let before = fs::symlink_metadata(path).expect("participant sidecar metadata");
    assert!(before.is_file() && !before.file_type().is_symlink() && before.nlink() == 1);
    assert!(before.len() <= 4096, "participant sidecar bound");
    let bytes = fs::read(path).expect("read deny-only participant sidecar");
    let after = fs::symlink_metadata(path).expect("participant sidecar post-read metadata");
    assert!(after.is_file() && !after.file_type().is_symlink() && after.nlink() == 1);
    assert_eq!(
        (before.dev(), before.ino(), before.len()),
        (after.dev(), after.ino(), after.len())
    );
    assert_eq!(bytes.len() as u64, after.len());
    let text = std::str::from_utf8(&bytes).expect("participant sidecar UTF-8");
    let lines: Vec<_> = text.lines().collect();
    assert!(
        lines.len() == 17 && lines[0] == "harflow-owner-participant-1",
        "participant sidecar shape"
    );
    assert_eq!(lines[2], support::OP, "participant operation binding");
    let fields: Vec<_> = lines[15].split(':').collect();
    assert_eq!(fields.len(), 4, "participant writer identity fields");
    ProcessIdentity::new(
        fields[0].parse().expect("writer PID"),
        fields[1].parse().expect("writer start seconds"),
        fields[2].parse().expect("writer start microseconds"),
        fields[3].to_owned(),
    )
    .expect("valid writer process identity")
}

pub(super) fn wait_writer_dead(writer: &ProcessIdentity, until: Instant) {
    loop {
        let evidence = MacRecoveryIdentity
            .observe(writer, until)
            .expect("fresh writer process observation");
        if matches!(
            evidence.state,
            ProcessState::Zombie
                | ProcessState::AbsentPid
                | ProcessState::PidReused
                | ProcessState::BootChanged
        ) {
            return;
        }
        assert!(
            Instant::now() < until,
            "writer process remains unresolved: {:?}",
            evidence.state
        );
        thread::yield_now();
    }
}

pub(super) fn wait_writer_dead_or_abort(writer: &ProcessIdentity, until: Instant) {
    loop {
        match MacRecoveryIdentity.observe(writer, until) {
            Ok(evidence)
                if matches!(
                    evidence.state,
                    ProcessState::Zombie
                        | ProcessState::AbsentPid
                        | ProcessState::PidReused
                        | ProcessState::BootChanged
                ) =>
            {
                return;
            }
            Ok(_) if Instant::now() < until => thread::yield_now(),
            _ => std::process::abort(),
        }
    }
}
