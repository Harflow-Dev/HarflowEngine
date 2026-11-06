#![cfg(all(target_os = "macos", target_arch = "aarch64"))]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

#[path = "prepared_launch/capture.rs"]
mod capture;
#[path = "common/fixture_guard.rs"]
mod fixture_guard;
#[path = "prepared_launch/ownership_race.rs"]
mod ownership_race;
#[path = "prepared_launch/raw_mutations.rs"]
mod raw_mutations;
#[path = "prepared_launch/support.rs"]
mod support;
use capture::*;
use hf_application::{prepared_launch::CaptureContext, recovery::RecoveryError};
use hf_domain::ownership::{ClaimResult, PrepareResult};
use hf_storage::{Limits, RootHold, Sha256Digest, Store};
use ownership_race::*;
use raw_mutations::*;
use std::fs;
use support::*;

#[test]
fn fixed_helper_handoff_captures_real_protocol_and_distinguishes_caller_from_owner() {
    let _guard = fixture_guard::acquire();
    let scratch = scratch();
    let _cwd = CurrentDirectory::enter(&scratch);
    let actual = actual_capture(&scratch);
    let owner = &actual.receipt.ownership.owner.as_ref().unwrap().process;
    assert_ne!(
        owner, &actual.caller,
        "detached owner identity must differ from original caller"
    );
    assert_eq!(&actual.receipt.invitation.process, owner);
    let context = || CaptureContext {
        invitation: &actual.receipt.invitation,
        request: &actual.request,
        source: &actual.source,
    };
    assert_eq!(
        actual.reader.read(context(), &Delivery(None), deadline(3)),
        Err(RecoveryError::Refused)
    );
    let delivery = Delivery(Some(actual.receipt.clone()));
    let bytes = assert_projection(
        &actual.reader,
        &actual.receipt.invitation,
        &actual.request,
        &actual.source,
        &delivery,
        &actual.request.attempt,
    );
    let effect = fs::read(scratch.join("r03-b2.effects")).unwrap();
    assert_eq!(
        effect.iter().filter(|b| **b == b'\n').count(),
        1,
        "one actual helper effect"
    );
    assert_raw_mutations(&actual, &scratch, &bytes);
    fs::remove_dir_all(scratch).unwrap();
}

#[test]
fn concurrent_actual_store_claims_of_hfpl1_have_one_winner_and_existing_cannot_reclaim() {
    let _guard = fixture_guard::acquire();
    let scratch = scratch();
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
    let ledger = ledger(&scratch);
    let pin = hf_platform::prepared_launch::observe_binary(&binary(), &Sha256Digest, deadline(3))
        .unwrap();
    let request = request(
        fake_process(500),
        &scratch.to_string_lossy(),
        pin,
        ledger,
        source,
    );
    assert!(request.payload.starts_with(b"HFPL\x01"));
    assert!(matches!(
        store.ownership_prepare(&request, deadline(3)).unwrap(),
        PrepareResult::Prepared(_)
    ));
    drop(store);

    let outcomes = concurrent_claims(&root, &request);
    let grants: Vec<_> = outcomes
        .iter()
        .filter_map(|(owner, result)| match result {
            ClaimResult::Granted(snapshot) => {
                assert_eq!(snapshot.owner.as_ref(), Some(owner));
                Some(owner.clone())
            }
            ClaimResult::NoGrant(_) => None,
        })
        .collect();
    assert_eq!(grants.len(), 1, "exactly one prepared request CAS wins");

    assert_existing_cannot_reclaim(&root, &scratch, &request, &outcomes);
    fs::remove_dir_all(scratch).unwrap();
}
