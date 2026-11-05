use super::support::*;

#[test]
fn raw_creation_requires_distinct_positive_identities_and_capture_uses_them() {
    let mut owner = spawned_coordinator();
    let identity = owner
        .begin(
            StorageCommand::CreateRaw,
            digest('c'),
            deadline(),
            Instant::now(),
        )
        .unwrap()
        .identity
        .clone();
    owner.send_complete(&identity, Instant::now()).unwrap();
    let invalid = RawIdentities {
        stdout: FileIdentity {
            device: 1,
            inode: 4,
        },
        stderr: FileIdentity {
            device: 1,
            inode: 4,
        },
    };
    assert_eq!(
        owner.acknowledge(
            ack(identity.clone(), 2, spawned_snapshot(), None, Some(invalid)),
            Instant::now()
        ),
        Err(CoordinatorError::InvalidAck)
    );
    assert!(owner.is_poisoned());
    assert_eq!(owner.pending().unwrap().identity, identity);
    assert!(owner.raw_files().is_none());

    let (mut qualified, binding, accepted) = established_coordinator();
    let mut mismatched = binding;
    mismatched.stdout.inode += 1;
    assert_eq!(
        qualified
            .begin(
                StorageCommand::CaptureBegin(Box::new(mismatched)),
                digest('c'),
                deadline(),
                Instant::now()
            )
            .unwrap_err(),
        CoordinatorError::InvalidState
    );
    assert_eq!(qualified.snapshot(), Some(&accepted));
    assert!(qualified.pending().is_none());
}
