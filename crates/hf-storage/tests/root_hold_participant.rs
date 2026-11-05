#![cfg(unix)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use common::{Fixture, deadline, default_limits};
use hf_application::lease::LeaseMode;
use hf_domain::{
    owner_runtime::StorageParticipant,
    ownership::{Digest, Id, OwnerClaim},
    process::ProcessIdentity,
};
use hf_storage::{Error, RootHold, Store};
use std::{fs, os::unix::fs::MetadataExt};

fn id(c: char) -> Id {
    Id::new(c.to_string().repeat(32)).unwrap()
}
fn digest(c: char) -> Digest {
    Digest::new(c.to_string().repeat(64)).unwrap()
}
fn process(pid: u32) -> ProcessIdentity {
    ProcessIdentity::new(pid, 100, 0, "abcdef01-2345-6789-abcd-ef0123456789".into()).unwrap()
}
fn owner() -> OwnerClaim {
    OwnerClaim {
        owner: id('6'),
        nonce: id('7'),
        process: process(41),
    }
}
fn participant(source: hf_domain::recovery::SourceStamp) -> StorageParticipant {
    StorageParticipant {
        source,
        operation: id('1'),
        attempt: id('2'),
        request_digest: digest('a'),
        configuration_digest: digest('b'),
        profile_digest: digest('c'),
        owner: owner(),
        writer: process(42),
        channel_nonce: id('3'),
    }
}

#[test]
fn root_hold_is_read_only_and_keeps_shared_exclusion_until_drop() {
    let fixture = Fixture::new();
    let root = fixture.path().join("store");
    drop(
        Store::initialize_fixture(
            &root,
            default_limits(),
            deadline(),
            common::lease::lease_port(),
        )
        .unwrap(),
    );
    let before = common::snapshot_tree(&root);
    let hold = RootHold::open(&root, deadline(), common::lease::lease_port()).unwrap();
    assert_eq!(hold.identity().schema_version, hf_storage::SCHEMA_VERSION);
    assert_eq!(common::snapshot_tree(&root), before);

    let lock = root.join("store.lock");
    let exclusive =
        common::lease::lease_port().acquire(&lock, LeaseMode::Exclusive, false, deadline());
    assert!(matches!(
        exclusive,
        Err(hf_application::lease::LeaseError::Busy)
    ));
    drop(hold);
    assert!(
        common::lease::lease_port()
            .acquire(&lock, LeaseMode::Exclusive, false, deadline())
            .is_ok()
    );
    assert_eq!(common::snapshot_tree(&root), before);
}

#[test]
fn participant_sidecar_is_create_new_bounded_single_link_and_source_bound() {
    let fixture = Fixture::new();
    let root = fixture.path().join("store");
    drop(
        Store::initialize_fixture(
            &root,
            default_limits(),
            deadline(),
            common::lease::lease_port(),
        )
        .unwrap(),
    );
    let hold = RootHold::open(&root, deadline(), common::lease::lease_port()).unwrap();
    let store = Store::open(
        &root,
        default_limits(),
        deadline(),
        common::lease::lease_port(),
    )
    .unwrap();
    let source = hold.source_stamp().clone();
    let record = participant(source.clone());
    let identity = store
        .create_storage_participant(&record, deadline())
        .unwrap();
    let path = root.join(format!(
        "owner-participant-{}.v1",
        record.operation.as_str()
    ));
    let metadata = fs::symlink_metadata(&path).unwrap();
    let bytes = fs::read(&path).unwrap();
    assert!(metadata.is_file());
    assert!(!metadata.file_type().is_symlink());
    assert_eq!(metadata.nlink(), 1);
    assert_eq!(metadata.mode() & 0o777, 0o600);
    assert_eq!(metadata.ino(), identity.file.inode);
    assert!(bytes.len() <= 4096);

    assert!(matches!(
        store.create_storage_participant(&record, deadline()),
        Err(Error::Io(_))
    ));
    assert_eq!(fs::read(&path).unwrap(), bytes);

    let mut wrong = record.clone();
    wrong.source.generation = id('9');
    assert!(matches!(
        store.create_storage_participant(&wrong, deadline()),
        Err(Error::Replaced)
    ));
    assert_eq!(fs::read(&path).unwrap(), bytes);
}
