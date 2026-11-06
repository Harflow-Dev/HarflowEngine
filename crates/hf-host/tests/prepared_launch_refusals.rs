#![cfg(all(target_os = "macos", target_arch = "aarch64"))]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

#[path = "common/fixture_guard.rs"]
mod fixture_guard;
#[path = "prepared_launch/support.rs"]
mod support;
use hf_domain::fixture::{FixtureFault, Frame, ParentPlan, ReportKind};
use hf_storage::{Limits, RootHold, Sha256Digest, Store};
use std::fs;
use support::*;

#[test]
fn fixed_preclaim_binary_cwd_root_and_config_mismatches_refuse_without_store_mutation() {
    let _guard = fixture_guard::acquire();
    let scratch = scratch();
    let _cwd = CurrentDirectory::enter(&scratch);
    let root = scratch.join("store");
    let store = Store::initialize_fixture(
        &root,
        Limits::default(),
        deadline(3),
        hf_platform::lease::shared(),
    )
    .unwrap();
    let source = RootHold::open(&root, deadline(3), hf_platform::lease::shared())
        .unwrap()
        .source_stamp()
        .clone();
    drop(store);
    let ledger = ledger(&scratch);
    for mismatch in 0..4 {
        let (_, frame) = run_parent(&scratch, |caller| {
            let mut pin =
                hf_platform::prepared_launch::observe_binary(&binary(), &Sha256Digest, deadline(3))
                    .unwrap();
            let mut cwd = scratch.to_string_lossy().into_owned();
            let mut source = source.clone();
            if mismatch == 0 {
                pin.digest = digest('c');
            }
            if mismatch == 1 {
                cwd = "/private/tmp".into();
            }
            if mismatch == 2 {
                source.root.inode += 1;
            }
            let mut request = request(caller, &cwd, pin, ledger, source);
            if mismatch == 3 {
                request = stale_configuration(request);
            }
            ParentPlan {
                root: root.to_string_lossy().into_owned(),
                request,
                initialize: false,
                launch: true,
                owner: id('6'),
                nonce: id('7'),
                gate: None,
                ledger,
                fault: FixtureFault::None,
            }
        });
        assert!(
            matches!(
                frame,
                Frame::Report {
                    kind: ReportKind::Uncertain,
                    ..
                }
            ),
            "mismatch {mismatch} must refuse before claim"
        );
        assert!(store_absent(&root));
    }
    assert!(fs::read(scratch.join("r03-b2.effects")).unwrap().is_empty());
    fs::remove_dir_all(scratch).unwrap();
}

#[test]
fn fixed_initialize_true_refuses_before_creating_the_store_root() {
    let _guard = fixture_guard::acquire();
    let scratch = scratch();
    let _cwd = CurrentDirectory::enter(&scratch);
    let root = scratch.join("store");
    let ledger = ledger(&scratch);
    let (_, frame) = run_parent(&scratch, |caller| {
        let pin =
            hf_platform::prepared_launch::observe_binary(&binary(), &Sha256Digest, deadline(3))
                .unwrap();
        let request = request(
            caller,
            &scratch.to_string_lossy(),
            pin,
            ledger,
            fake_source(),
        );
        ParentPlan {
            root: root.to_string_lossy().into_owned(),
            request,
            initialize: true,
            launch: true,
            owner: id('6'),
            nonce: id('7'),
            gate: None,
            ledger,
            fault: FixtureFault::None,
        }
    });
    assert!(matches!(
        frame,
        Frame::Report {
            kind: ReportKind::Uncertain,
            ..
        }
    ));
    assert!(
        !root.exists(),
        "fixed initialization refusal must precede root creation"
    );
    assert!(fs::read(scratch.join("r03-b2.effects")).unwrap().is_empty());
    fs::remove_dir_all(scratch).unwrap();
}

fn store_absent(root: &std::path::Path) -> bool {
    Store::open(
        root,
        Limits::default(),
        deadline(2),
        hf_platform::lease::shared(),
    )
    .unwrap()
    .ownership_observe(&id('1'), deadline(2))
    .unwrap()
    .is_none()
}
