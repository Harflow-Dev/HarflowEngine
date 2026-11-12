#![cfg(unix)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

//! The check of every ordinary record that the direct-only caller gate runs: damage,
//! coverage, unusable identities, and the read budget it charges.
mod common;
mod fixture_support;
mod pending_support;
use fixture_support::{A, B, done, later, tid, world};
use hf_application::ctl::task_reads::PendingLimits;
use hf_domain::task_read::Coverage;
use hf_storage::TaskFixture;
use pending_support::{checked, checked_within, launch, put};

/// `header:<id>` and `status:<id>` are 43 bytes each, charged by their raw stored length.
const RECORD: u64 = 43 + 43;

#[test]
fn a_root_without_records_is_checked_even_with_no_budget() {
    let w = world();
    assert!(checked(&w, 0));
}

#[test]
fn launch_rows_are_not_ordinary_records() {
    let w = world();
    put(&w, &launch(A, "reserved"));
    assert!(checked(&w, 0));
}

#[test]
fn valid_records_pass_and_charge_their_stored_documents() {
    let w = world();
    w.add(&done(A));
    assert!(checked(&w, RECORD));
    assert!(!checked(&w, RECORD - 1));
    w.add(&done(B));
    assert!(checked(&w, 2 * RECORD));
    assert!(!checked(&w, 2 * RECORD - 1));
}

#[test]
fn a_damaged_record_makes_the_check_fail() {
    let w = world();
    w.add(&done(A));
    w.add(&done(B));
    w.store.damage_task_fixture(&tid(B), later(5)).unwrap();
    assert!(!checked(&w, u64::MAX));
}

#[test]
fn a_record_without_a_fact_row_makes_the_check_fail() {
    let w = world();
    w.add(&done(A));
    let db = w.raw();
    db.execute_batch("PRAGMA foreign_keys=OFF").unwrap();
    db.execute("DELETE FROM task_facts WHERE id=?1", [A])
        .unwrap();
    drop(db);
    assert!(!checked(&w, u64::MAX));
}

#[test]
fn a_record_whose_id_is_not_a_task_identity_makes_the_check_fail() {
    let w = world();
    w.add(&done(A));
    let db = w.raw();
    db.execute_batch("PRAGMA foreign_keys=OFF").unwrap();
    let changed = db
        .execute("UPDATE task_headers SET id='bad id!' WHERE id=?1", [A])
        .unwrap();
    drop(db);
    assert_eq!(changed, 1);
    assert!(!checked(&w, u64::MAX));
}

#[test]
fn incomplete_dataset_coverage_makes_the_check_fail() {
    for (discovery, ownership, source) in [
        (false, true, true),
        (true, false, true),
        (true, true, false),
    ] {
        let w = world();
        w.store
            .replace_task_fixture(
                &TaskFixture {
                    rows: Vec::new(),
                    launches: Vec::new(),
                    coverage: Coverage {
                        discovery_complete: discovery,
                        ownership_complete: ownership,
                        source_complete: source,
                    },
                },
                later(5),
            )
            .unwrap();
        assert!(!checked(&w, u64::MAX), "{discovery} {ownership} {source}");
    }
}

#[test]
fn records_and_receipts_share_the_entry_and_name_bounds() {
    let w = world();
    w.add(&done(A));
    put(&w, &launch(B, "reserved"));
    let fits = PendingLimits {
        entries: 2,
        name_bytes: A.len() + ".meta.json".len() + B.len() + ".launch.json".len(),
        read_bytes: 0,
    };
    assert!(checked_within(&w, u64::MAX, &fits));
    let few = PendingLimits { entries: 1, ..fits };
    assert!(!checked_within(&w, u64::MAX, &few));
    let narrow = PendingLimits {
        name_bytes: fits.name_bytes - 1,
        ..fits
    };
    assert!(!checked_within(&w, u64::MAX, &narrow));
}

#[test]
fn a_deadline_that_passes_in_the_check_is_unchecked_and_the_session_exit_then_reports_it() {
    use hf_storage::Error;
    let w = world();
    w.add(&done(A));
    let deadline = std::time::Instant::now() + std::time::Duration::from_millis(150);
    let outcome = w.session().snapshot(deadline, |s| {
        std::thread::sleep(std::time::Duration::from_millis(300));
        s.candidates_checked(
            &hf_application::ctl::task_reads::PendingBudget::new(u64::MAX),
            &PendingLimits::default(),
        )
    });
    // The check itself never reports a person; the exit check of the session is what fails.
    assert!(
        matches!(outcome, Err(Error::DeadlineExceeded)),
        "{outcome:?}"
    );
}
