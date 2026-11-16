#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

//! Proposals of `state-inspect` for roots with a recovery journal, non-regular reserved
//! files, and how a proposal is rendered.
mod ctl_state_proposal_support;

use ctl_state_proposal_support::*;

// ---- non-regular infrastructure and journals ----------------------------------------

#[test]
fn non_regular_marker_lock_or_journal_has_no_proposal() {
    let cases: [(&str, ChangeFacts); 5] = [
        ("marker", |f| f.python_marker = FileState::NotRegular),
        ("journal", |f| f.journal = FileState::NotRegular),
        ("journal pending", |f| {
            f.journal_pending = FileState::NotRegular
        }),
        ("lock directory", |f| {
            f.entries.push(RootEntry {
                name: ".harflow-store.lock".to_string(),
                kind: EntryKind::Directory,
                empty_directory: true,
            });
        }),
        ("pending marker", |f| {
            f.entries.push(RootEntry {
                name: ".harflow-store.json.pending".to_string(),
                kind: EntryKind::Other,
                empty_directory: false,
            });
        }),
    ];
    for (name, change) in cases {
        let inspection = Case::stray().with(change).inspect();
        assert_eq!(inspection.reason, InspectReason::InvalidMarker, "{name}");
        assert!(inspection.recovery.is_none(), "{name}");
    }
}

// A store-state file that is a directory or a symbolic link (read as `NotRegular`) is walked
// like any other entry, so it keeps its proposal and the verdict does not change. The
// excluded names (the recovery record among them) are refused by the inventory walk
// itself, which is covered in the platform tests.
#[test]
fn non_regular_store_state_file_keeps_its_proposal_and_verdict() {
    for name in [
        "store_identity",
        "database_header",
        "wal_acceptance",
        "fence",
        "watermark",
    ] {
        let (_, set) = RESERVED.iter().find(|(n, _)| *n == name).unwrap();
        let with_walk = Case::stray()
            .with(|facts| set(facts, FileState::NotRegular))
            .inspect();
        let without = Case::stray()
            .with(|facts| set(facts, FileState::NotRegular))
            .walking(Err(InventoryFailure::Special))
            .inspect();
        assert!(with_walk.recovery.is_some(), "{name}");
        assert_eq!(verdict(&with_walk), verdict(&without), "{name}");
    }
}

#[test]
fn a_walk_refusing_a_non_regular_recovery_record_gives_no_proposal() {
    let inspection = Case::stray()
        .with(|facts| facts.recovery_record = FileState::NotRegular)
        .walking(Err(InventoryFailure::Special))
        .inspect();
    assert_eq!(inspection.reason, InspectReason::InvalidMarker);
    assert!(inspection.recovery.is_none());
}

#[test]
fn an_interrupted_journal_of_this_engine_is_resumable_with_its_phase_and_proposal() {
    for phase in PHASES {
        for pending in [false, true] {
            let (inspection, walks) = Case::stray()
                .with(|facts| {
                    let state = FileState::Bytes(journal_bytes(phase, PRODUCT_VERSION));
                    if pending {
                        facts.journal_pending = state;
                    } else {
                        facts.journal = state;
                    }
                })
                .run();
            assert_eq!(
                verdict(&inspection),
                (
                    InspectStatus::Interrupted,
                    InspectReason::RecoveryInterrupted
                ),
                "{phase}"
            );
            let offer = offered(&inspection);
            assert!(offer.resumable, "{phase}");
            assert_eq!(offer.phase, phase);
            // The journal's own proposal, operation id and all; nothing is generated.
            assert_eq!(offer.proposal, journal_proposal(PRODUCT_VERSION));
            assert_eq!(walks, 0, "{phase}: a resume does not walk");
        }
    }
}

#[test]
fn other_engine_version_journal_has_no_proposal() {
    for version in ["0.0.14", "0.0.13", "0.0.13+python", "0.0.13+RUST", ""] {
        for phase in PHASES {
            let inspection = Case::stray()
                .with(|facts| {
                    facts.journal = FileState::Bytes(journal_bytes(phase, version));
                })
                .inspect();
            assert_eq!(
                inspection.status,
                InspectStatus::Interrupted,
                "{version} {phase}"
            );
            assert!(inspection.recovery.is_none(), "{version:?} {phase}");
        }
        // A finished journal that nothing committed is refused, and also not proposed.
        let inspection = Case::stray()
            .with(|facts| facts.journal = FileState::Bytes(journal_bytes("completed", version)))
            .inspect();
        assert_eq!(
            verdict(&inspection),
            (
                InspectStatus::Incompatible,
                InspectReason::RecoveryInterrupted
            )
        );
        assert!(inspection.recovery.is_none(), "{version:?} completed");
    }
}

#[test]
fn a_journal_without_a_decodable_proposal_has_no_proposal() {
    let mut document: Value =
        serde_json::from_slice(&journal_bytes("planned", PRODUCT_VERSION)).unwrap();
    document["proposal"] =
        json!({"operation_id": JOURNAL_ID, "state_root": ROOT, "root_identity": [7, 100]});
    let bytes = serde_json::to_vec(&document).unwrap();
    let inspection = Case::stray()
        .with(|facts| facts.journal = FileState::Bytes(bytes))
        .inspect();
    assert_eq!(inspection.status, InspectStatus::Interrupted);
    assert!(inspection.recovery.is_none());
}

#[test]
fn unqualified_or_unparseable_journals_have_no_proposal() {
    let mut other_root: Value =
        serde_json::from_slice(&journal_bytes("planned", PRODUCT_VERSION)).unwrap();
    other_root["state_root"] = json!("/somewhere/else");
    for (label, bytes) in [
        ("other root", serde_json::to_vec(&other_root).unwrap()),
        ("garbage", b"\x00not json".to_vec()),
        ("array", b"[1]".to_vec()),
    ] {
        let inspection = Case::stray()
            .with(|facts| facts.journal = FileState::Bytes(bytes))
            .inspect();
        assert!(inspection.recovery.is_none(), "{label}");
    }
}

#[test]
fn a_finished_journal_of_this_engine_that_nothing_committed_is_proposed_afresh() {
    let bytes = journal_bytes("completed", PRODUCT_VERSION);
    let inspection = Case::stray()
        .with(|facts| facts.journal = FileState::Bytes(bytes.clone()))
        .inspect();
    assert_eq!(
        verdict(&inspection),
        (
            InspectStatus::Incompatible,
            InspectReason::RecoveryInterrupted
        )
    );
    let offer = offered(&inspection);
    assert!(!offer.resumable);
    assert_eq!(offer.phase, PHASE_PROPOSED);
    assert_eq!(offer.proposal.operation_id, FRESH_ID);
    assert_eq!(
        offer.proposal.prior_journal_sha256.as_deref(),
        Some(stand_in(&[&bytes]).as_str())
    );
}

#[test]
fn only_the_journal_file_is_the_prior_journal() {
    // A publication temporary is read for classification but is not the prior journal.
    let bytes = journal_bytes("completed", PRODUCT_VERSION);
    let inspection = Case::stray()
        .with(|facts| facts.journal_pending = FileState::Bytes(bytes))
        .inspect();
    assert_eq!(offered(&inspection).proposal.prior_journal_sha256, None);
}

// ---- rendering ---------------------------------------------------------------------

#[test]
fn recovery_supported_is_true_exactly_where_a_proposal_is_given() {
    let cases = [
        ("proposed", Case::stray(), true),
        (
            "walk failed",
            Case::stray().walking(Err(InventoryFailure::Changed)),
            false,
        ),
        (
            "ready",
            Case::new(RootFacts::without_files(RootStat::Present)),
            false,
        ),
        (
            "missing",
            Case::new(RootFacts::without_files(RootStat::Missing)),
            false,
        ),
        (
            "resumable",
            Case::stray()
                .with(|f| f.journal = FileState::Bytes(journal_bytes("prepared", PRODUCT_VERSION))),
            true,
        ),
    ];
    for (label, case, supported) in cases {
        let inspection = case.inspect();
        let document = render::inspection(&inspection);
        assert_eq!(document["recovery_supported"], json!(supported), "{label}");
        assert_eq!(document.get("recovery").is_some(), supported, "{label}");
    }
}

#[test]
fn the_rendered_recovery_object_carries_a_token_for_the_offered_proposal() {
    let inspection = Case::stray().inspect();
    let document = render::inspection(&inspection);
    let recovery = document["recovery"].as_object().unwrap();
    let mut keys: Vec<_> = recovery.keys().map(String::as_str).collect();
    keys.sort_unstable();
    assert_eq!(
        keys,
        [
            "backup_path",
            "confirmation_token",
            "operation_id",
            "phase",
            "resumable"
        ]
    );
    assert_eq!(recovery["operation_id"], json!(FRESH_ID));
    assert_eq!(recovery["phase"], json!("proposed"));
    assert_eq!(recovery["resumable"], json!(false));
    assert_eq!(
        recovery["backup_path"],
        json!(format!("{ROOT}.harflow-backup-{FRESH_ID}"))
    );
    let token = recovery["confirmation_token"].as_str().unwrap();
    assert_eq!(
        recovery_codec::decode_token(token).as_ref(),
        Ok(&offered(&inspection).proposal)
    );
}

#[test]
fn the_rendered_resumable_recovery_reports_the_journal_phase() {
    let inspection = Case::stray()
        .with(|f| f.journal = FileState::Bytes(journal_bytes("backed_up", PRODUCT_VERSION)))
        .inspect();
    let document = render::inspection(&inspection);
    let recovery = &document["recovery"];
    assert_eq!(recovery["phase"], json!("backed_up"));
    assert_eq!(recovery["resumable"], json!(true));
    assert_eq!(recovery["operation_id"], json!(JOURNAL_ID));
    assert_eq!(
        recovery["confirmation_token"],
        json!(recovery_codec::encode_token(&journal_proposal(
            PRODUCT_VERSION
        )))
    );
    assert_eq!(document["status"], json!("interrupted"));
}

// ---- a resume runs the facts-only eligibility checks ----------------------------------

#[test]
fn a_resumable_journal_with_an_ineligible_root_has_no_proposal_and_the_same_verdict() {
    let conditions: [(&str, ChangeFacts); 5] = [
        ("unreadable identity", |f| {
            f.store_identity = FileState::Unreadable
        }),
        ("unreadable fence", |f| f.fence = FileState::Unreadable),
        ("unreadable record", |f| {
            f.recovery_record = FileState::Unreadable
        }),
        ("non-regular lock", |f| {
            f.entries.push(RootEntry {
                name: ".harflow-store.lock".to_string(),
                kind: EntryKind::Directory,
                empty_directory: true,
            });
        }),
        ("valid python marker", |f| {
            f.python_marker = FileState::Bytes(marker("harflow"))
        }),
    ];
    for (name, change) in conditions {
        for phase in PHASES {
            let ours = Case::stray()
                .with(change)
                .with(|f| f.journal = FileState::Bytes(journal_bytes(phase, PRODUCT_VERSION)))
                .inspect();
            let other = Case::stray()
                .with(change)
                .with(|f| f.journal = FileState::Bytes(journal_bytes(phase, "0.0.14")))
                .inspect();
            assert!(ours.recovery.is_none(), "{name} {phase}");
            assert_eq!(verdict(&ours), verdict(&other), "{name} {phase}");
        }
    }
}
