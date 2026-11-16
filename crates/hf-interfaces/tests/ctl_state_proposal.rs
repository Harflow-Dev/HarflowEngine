#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

//! Proposal eligibility of `state-inspect`, one observed fact at a time, with the real
//! codecs and fake effects (observer, inventory walk, digest and operation ids).
mod ctl_state_proposal_support;

use ctl_state_proposal_support::*;

// ---- the proposal itself -----------------------------------------------------------

#[test]
fn an_eligible_root_is_proposed_with_the_walk_and_the_ports() {
    let (inspection, walks) = Case::stray().run();
    assert_eq!(
        verdict(&inspection),
        (InspectStatus::Incompatible, InspectReason::UnmarkedNonempty)
    );
    assert_eq!(walks, 1);
    let offer = offered(&inspection);
    assert_eq!(offer.phase, PHASE_PROPOSED);
    assert!(!offer.resumable);
    let proposal = &offer.proposal;
    assert_eq!(proposal.state_root, ROOT);
    assert_eq!(proposal.engine_version, PRODUCT_VERSION);
    assert_eq!(proposal.root_identity, FileIdentity { dev: 7, ino: 100 });
    assert_eq!(
        proposal.lock_identity,
        Some(FileIdentity { dev: 7, ino: 101 })
    );
    assert_eq!(proposal.operation_id, FRESH_ID);
    assert_eq!(proposal.prior_journal_sha256, None);
    // The digest is over the canonical inventory bytes, not over the proposal.
    let bytes = recovery_codec::encode_inventory(&inventory()).into_bytes();
    assert_eq!(proposal.inventory_sha256, stand_in(&[&bytes]).as_str());
    assert_eq!(
        proposal.backup_path(),
        format!("{ROOT}.harflow-backup-{FRESH_ID}")
    );
}

#[test]
fn a_root_without_a_lock_has_no_lock_identity() {
    let mut walked = inventory();
    walked.lock = None;
    let inspection = Case::stray().walking(Ok(walked)).inspect();
    assert_eq!(offered(&inspection).proposal.lock_identity, None);
}

#[test]
fn no_operation_id_means_no_proposal() {
    let mut case = Case::stray();
    case.ids = None;
    let inspection = case.inspect();
    assert!(inspection.recovery.is_none());
    assert_eq!(inspection.reason, InspectReason::UnmarkedNonempty);
}

#[test]
fn a_proposal_changes_neither_status_nor_reason() {
    let with_walk = Case::stray().inspect();
    let without = Case::stray()
        .walking(Err(InventoryFailure::Special))
        .inspect();
    assert_eq!(verdict(&with_walk), verdict(&without));
    assert_eq!(with_walk.observed_store, without.observed_store);
    assert!(with_walk.recovery.is_some() && without.recovery.is_none());
}

// ---- one test per eligibility fact, both ways ---------------------------------------

#[test]
fn fact_root_exists() {
    let (missing, walks) = Case::new(RootFacts::without_files(RootStat::Missing)).run();
    assert_eq!(
        verdict(&missing),
        (InspectStatus::Missing, InspectReason::MissingRoot)
    );
    assert!(missing.recovery.is_none());
    assert_eq!(walks, 0, "a missing root is never walked");
    assert!(Case::stray().inspect().recovery.is_some());
}

#[test]
fn fact_root_is_safe() {
    for stat in [RootStat::Unsafe, RootStat::Unreadable] {
        let mut facts = stray_facts();
        facts.stat = stat;
        let (inspection, walks) = Case::new(facts).run();
        assert_eq!(inspection.status, InspectStatus::Incompatible);
        assert!(inspection.recovery.is_none(), "{stat:?}");
        assert_eq!(walks, 0, "{stat:?}");
    }
    assert!(Case::stray().inspect().recovery.is_some());
}

#[test]
fn fact_no_python_marker() {
    let python = Case::stray()
        .with(|facts| facts.python_marker = FileState::Bytes(marker("harflow")))
        .run();
    assert_eq!(python.0.reason, InspectReason::UnsupportedLayout);
    assert!(python.0.recovery.is_none());
    assert_eq!(python.1, 0);
    // Markers that are not a store of the reference engine stay recoverable.
    for (label, bytes, reason) in [
        ("foreign", marker("elsewhere"), InspectReason::ForeignStore),
        (
            "invalid",
            b"{not json".to_vec(),
            InspectReason::InvalidMarker,
        ),
    ] {
        let inspection = Case::stray()
            .with(|facts| facts.python_marker = FileState::Bytes(bytes))
            .inspect();
        assert_eq!(inspection.reason, reason, "{label}");
        assert!(inspection.recovery.is_some(), "{label}");
    }
}

#[test]
fn fact_no_reference_state_directories() {
    for name in ["tasks", "workflow-runs", "workflow-leases"] {
        let mut walked = inventory();
        walked
            .entries
            .insert(name.to_string(), directory(&[("record", file())]));
        let inspection = Case::stray().walking(Ok(walked)).inspect();
        assert!(inspection.recovery.is_none(), "{name} with data");
        assert_eq!(inspection.reason, InspectReason::UnmarkedNonempty, "{name}");
        // The same directory without content carries no state.
        let mut walked = inventory();
        walked.entries.insert(name.to_string(), directory(&[]));
        assert!(
            Case::stray()
                .walking(Ok(walked))
                .inspect()
                .recovery
                .is_some(),
            "{name} empty"
        );
    }
    // A file of that name, or the directory nested deeper, is not reference state.
    let mut walked = inventory();
    walked.entries.insert("tasks".to_string(), file());
    walked.entries.insert(
        "nested".to_string(),
        directory(&[("tasks", directory(&[("record", file())]))]),
    );
    assert!(
        Case::stray()
            .walking(Ok(walked))
            .inspect()
            .recovery
            .is_some()
    );
}

#[test]
fn fact_walk_completed_without_change_within_the_bound() {
    for failure in [
        InventoryFailure::TooLarge,
        InventoryFailure::Special,
        InventoryFailure::Changed,
        InventoryFailure::Unreadable,
        InventoryFailure::Unsupported,
    ] {
        let (inspection, walks) = Case::stray().walking(Err(failure)).run();
        assert_eq!(walks, 1, "{failure:?}");
        assert!(inspection.recovery.is_none(), "{failure:?}");
        assert_eq!(
            verdict(&inspection),
            (InspectStatus::Incompatible, InspectReason::UnmarkedNonempty),
            "{failure:?} leaves the verdict alone"
        );
    }
    assert!(Case::stray().inspect().recovery.is_some());
}

#[test]
fn fact_every_reserved_file_was_readable() {
    for (name, set) in RESERVED {
        let (inspection, walks) = Case::stray()
            .with(|facts| set(facts, FileState::Unreadable))
            .run();
        assert_eq!(inspection.status, InspectStatus::Incompatible, "{name}");
        assert!(inspection.recovery.is_none(), "{name} unreadable");
        assert_eq!(walks, 0, "{name}: no walk for an unreadable file");
    }
}

#[test]
fn unreadable_reserved_file_has_no_proposal_on_a_store_state_root() {
    // A stray root where only the store-state file is unreadable, and the same root with
    // that file readable but not a valid document: the second is recoverable.
    for (name, set) in RESERVED {
        let readable = Case::stray()
            .with(|facts| set(facts, FileState::Bytes(b"garbage".to_vec())))
            .inspect();
        let unreadable = Case::stray()
            .with(|facts| set(facts, FileState::Unreadable))
            .inspect();
        assert!(unreadable.recovery.is_none(), "{name}");
        // Garbage in a journal is not a recoverable state of this engine; the others are.
        if !matches!(name, "journal" | "journal_pending") {
            assert!(readable.recovery.is_some(), "{name} readable");
        }
    }
}

#[test]
fn fact_verdict_is_not_ready_or_missing() {
    // An empty root is ready, and so is the admissible set of empty legacy directories.
    let (ready, walks) = Case::new(RootFacts::without_files(RootStat::Present)).run();
    assert_eq!(
        verdict(&ready),
        (InspectStatus::Ready, InspectReason::Compatible)
    );
    assert!(ready.recovery.is_none());
    assert_eq!(walks, 0, "a ready root is never walked");
    let mut legacy = RootFacts::without_files(RootStat::Present);
    legacy.entries = [
        "workflows",
        "workflow-runs",
        "workflow-leases",
        "workflow-owners",
    ]
    .map(|name| RootEntry {
        name: name.to_string(),
        kind: EntryKind::Directory,
        empty_directory: true,
    })
    .to_vec();
    let (ready, walks) = Case::new(legacy).run();
    assert_eq!(ready.status, InspectStatus::Ready);
    assert!(ready.recovery.is_none());
    assert_eq!(walks, 0);
    assert!(Case::stray().inspect().recovery.is_some());
}

// ---- the proposal is bound to the directory the facts were read from ------------------

#[test]
fn a_walked_root_that_is_not_the_observed_root_has_no_proposal() {
    let mut other = inventory();
    other.root = FileIdentity { dev: 7, ino: 999 };
    let swapped = Case::stray().walking(Ok(other)).inspect();
    assert!(swapped.recovery.is_none());
    let same = Case::stray().inspect();
    assert_eq!(verdict(&swapped), verdict(&same));
    let mut unknown = Case::stray();
    unknown.facts.root_id = None;
    assert!(unknown.inspect().recovery.is_none());
}

#[test]
fn a_marker_that_appears_in_the_inventory_after_the_facts_gives_no_proposal() {
    let mut walked = inventory();
    walked
        .entries
        .insert(".harflow-store.json".to_string(), file());
    let inspection = Case::stray().walking(Ok(walked)).inspect();
    assert!(inspection.recovery.is_none());
    assert_eq!(
        verdict(&inspection),
        verdict(&Case::stray().inspect()),
        "the verdict is not changed"
    );
}
