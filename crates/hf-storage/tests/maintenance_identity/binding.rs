use super::*;
use hf_application::ctl::state_recover::binding::{BindingMode, verify_binding};
use hf_domain::ctl::{
    identity::PRODUCT_VERSION,
    recovery_proposal::{FileIdentity as RecoveryFileIdentity, RecoveryProposal},
};

#[test]
fn legacy_lock_appearing_after_acquisition_is_active_owner() {
    let scratch = Fixture::new();
    let root = store(&scratch);
    let session = acquire_with(&root, &request(&root, None), common::lease::lease_port()).unwrap();
    fs::write(root.join(".harflow-store.lock"), b"").unwrap();
    assert_eq!(session.verify_held(), Err(BindingError::ActiveOwner));
}

#[test]
fn real_maintenance_session_verifies_fresh_null_and_resume_lock_bindings() {
    let scratch = Fixture::new();
    let root = store(&scratch);
    fs::remove_file(root.join("store.lock")).unwrap();
    let session = acquire_with(&root, &request(&root, None), common::lease::lease_port()).unwrap();
    let root_identity = RecoveryFileIdentity {
        dev: session.root_identity().device as i64,
        ino: session.root_identity().inode,
    };
    let lock_identity = RecoveryFileIdentity {
        dev: session.lock_identity().device as i64,
        ino: session.lock_identity().inode,
    };
    let proposal = RecoveryProposal {
        state_root: root.to_str().unwrap().into(),
        engine_version: PRODUCT_VERSION.into(),
        root_identity,
        lock_identity: None,
        inventory_sha256: "a".repeat(64),
        operation_id: "0123456789abcdef0123456789abcdef".into(),
        prior_journal_sha256: None,
    };
    let selected_root = root.to_str().unwrap();
    assert_eq!(
        verify_binding(
            &session,
            &proposal,
            selected_root,
            PRODUCT_VERSION,
            &proposal.operation_id,
            BindingMode::Fresh
        ),
        Ok(())
    );
    assert_eq!(
        verify_binding(
            &session,
            &proposal,
            selected_root,
            PRODUCT_VERSION,
            &proposal.operation_id,
            BindingMode::Resume { lock_identity }
        ),
        Ok(())
    );
    assert_eq!(
        verify_binding(
            &session,
            &proposal,
            selected_root,
            PRODUCT_VERSION,
            &proposal.operation_id,
            BindingMode::Resume {
                lock_identity: RecoveryFileIdentity {
                    ino: 1,
                    ..lock_identity
                }
            }
        ),
        Err(BindingError::Stale)
    );
}

#[test]
fn held_root_locks_are_reverified_after_removal_and_replacement() {
    for name in ["store.lock", "io.lock"] {
        for replace in [false, true] {
            let scratch = Fixture::new();
            let root = store(&scratch);
            let session = acquire_with(
                &root,
                &request(&root, Some(id(&root.join("store.lock")))),
                common::lease::lease_port(),
            )
            .unwrap();
            let path = root.join(name);
            if replace {
                let held = root.join(format!("{name}.held"));
                fs::rename(&path, &held).unwrap();
                fs::write(&path, b"replacement").unwrap();
            } else {
                fs::remove_file(&path).unwrap();
            }
            assert_eq!(session.verify_held(), Err(BindingError::Stale), "{name}");
        }
    }
}

#[test]
fn legacy_lock_removal_or_inode_replacement_after_acquisition_is_stale() {
    for replace in [false, true] {
        let scratch = Fixture::new();
        let root = store(&scratch);
        let legacy = root.join(".harflow-store.lock");
        fs::write(&legacy, b"legacy").unwrap();
        let session =
            acquire_with(&root, &request(&root, None), common::lease::lease_port()).unwrap();
        if replace {
            let moved = root.join(".harflow-store.lock.held");
            fs::rename(&legacy, &moved).unwrap();
            fs::write(&legacy, b"replacement").unwrap();
        } else {
            fs::remove_file(&legacy).unwrap();
        }
        assert_eq!(session.verify_held(), Err(BindingError::Stale));
    }
}
