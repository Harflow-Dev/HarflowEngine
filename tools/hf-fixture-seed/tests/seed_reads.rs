#![cfg(all(target_os = "macos", target_arch = "aarch64"))]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

//! The seeded conformance world, read back through `ReadSession`: task pages, activity
//! and prefix resolution.
mod support;
use hf_domain::task_read::{Coverage, Lookup, StoredObservation};
use support::{
    A, A_STAMP, B, C, D, DAY, E, MISSING, Scratch, ids, lookup, ok, page, seeded, serial, task, tid,
};

fn settled(status: &str, exited: bool) -> StoredObservation {
    StoredObservation {
        recorded_status: Some(status.into()),
        active: Some(false),
        exited: Some(exited),
        drained: Some(true),
        owned: Some(false),
    }
}

#[test]
fn the_world_pages_newest_first_with_exact_stamps_sessions_and_observations() {
    let _serial = serial();
    let scratch = seeded();
    let result = page(&scratch.root, 100, false, None, None);
    assert_eq!(ids(&result), [E, D, C, B, A]);
    let stamps: Vec<i64> = result.rows.iter().map(|r| r.stamp).collect();
    assert_eq!(
        stamps,
        [
            A_STAMP + 4 * DAY,
            A_STAMP + 3 * DAY,
            A_STAMP + 2 * DAY,
            A_STAMP + DAY,
            A_STAMP
        ]
    );
    let sessions: Vec<_> = result
        .rows
        .iter()
        .map(|r| r.session.clone().unwrap())
        .collect();
    assert_eq!(
        sessions,
        [
            "session-eeeeeeee",
            "session-dddddddd",
            "session-cccccccc",
            "session-bbbbbbbb",
            "session-aaaaaaaa"
        ]
    );
    let observations: Vec<_> = result.rows.iter().map(|r| r.observation.clone()).collect();
    assert_eq!(
        observations,
        [
            settled("completed", true),
            settled("completed", true),
            settled("completed", true),
            settled("failed", true),
            settled("completed", true)
        ]
    );
    assert!(result.activity_complete);
    assert!(!result.partial);
    assert_eq!(result.cursor, None);
    assert_eq!(result.count, Some(5));
    assert_eq!(
        result.coverage,
        Coverage {
            discovery_complete: true,
            ownership_complete: true,
            source_complete: true
        }
    );
}

#[test]
fn the_served_header_and_status_blobs_carry_the_seeded_values() {
    let _serial = serial();
    let scratch = seeded();
    let result = lookup(&scratch.root, &[A, C]);
    let header = |i: usize| String::from_utf8(result.rows[i].header.clone()).unwrap();
    let status = |i: usize| String::from_utf8(result.rows[i].status.clone().unwrap()).unwrap();
    for fragment in [
        r#""backend":"claude""#,
        r#""freedom":"write_in_repo""#,
        r#""group":"g1""#,
        r#""model":"model-a""#,
        r#""title":"First task""#,
        r#""repo_path":"/workspace/repo""#,
        r#""continuation_of_task_id":null"#,
    ] {
        assert!(header(0).contains(fragment), "{fragment} in {}", header(0));
    }
    assert!(header(1).contains(&format!(r#""continuation_of_task_id":"{A}""#)));
    assert!(header(1).contains(&format!(r#""chain_root_task_id":"{A}""#)));
    for fragment in [
        r#""status":"completed""#,
        r#""exit_code":0"#,
        r#""finished_at":"2026-01-10T10:05:00+00:00""#,
        r#""output_drained":true"#,
        r#""recovered":true"#,
        r#""observation_mode":"unknown""#,
        r#""needs_reconciliation":false"#,
        r#""process_identity_state":"dead""#,
    ] {
        assert!(status(0).contains(fragment), "{fragment} in {}", status(0));
    }
}

#[test]
fn paging_with_a_limit_walks_the_history_without_overlap() {
    let _serial = serial();
    let scratch = seeded();
    let first = page(&scratch.root, 2, false, None, None);
    assert_eq!((ids(&first), first.partial), (vec![E, D], true));
    let second = page(
        &scratch.root,
        2,
        false,
        None,
        Some(first.cursor.clone().unwrap()),
    );
    assert_eq!((ids(&second), second.partial), (vec![C, B], true));
    let third = page(
        &scratch.root,
        2,
        false,
        None,
        Some(second.cursor.clone().unwrap()),
    );
    assert_eq!(
        (ids(&third), third.partial, third.cursor.clone()),
        (vec![A], false, None)
    );
}

#[test]
fn active_only_lists_no_settled_task_and_a_running_caller_once_it_exists() {
    let _serial = serial();
    let scratch = seeded();
    let none = page(&scratch.root, 100, true, None, None);
    assert_eq!((ids(&none), none.activity_complete), (vec![], true));
    let caller = format!(
        r#"{{"task_id":"{MISSING}","pid":{},"identity":"unknown","status":"running","marker":null}}"#,
        std::process::id()
    );
    assert_eq!(
        ok(&scratch.root, "add_live_caller", &caller),
        "{\"supported\":true}\n"
    );
    let live = page(&scratch.root, 100, true, None, None);
    assert_eq!(ids(&live), [MISSING]);
    assert_eq!(
        live.rows[0].observation,
        StoredObservation {
            recorded_status: Some("running".into()),
            active: Some(true),
            exited: Some(false),
            drained: Some(false),
            owned: Some(false)
        }
    );
    assert_eq!(live.rows[0].stamp, 1_794_009_600_000_000);
    assert!(live.activity_complete);
    let all = page(&scratch.root, 100, false, None, None);
    assert_eq!(
        ids(&all),
        [MISSING, E, D, C, B, A],
        "the caller's 2026-11 start is the newest"
    );
}

#[test]
fn the_session_filter_selects_the_recorded_session() {
    let _serial = serial();
    let scratch = seeded();
    assert_eq!(
        ids(&page(
            &scratch.root,
            100,
            false,
            Some("session-cccccccc"),
            None
        )),
        [C]
    );
    assert_eq!(
        ids(&page(
            &scratch.root,
            100,
            false,
            Some("session-unknown"),
            None
        )),
        Vec::<&str>::new()
    );
}

#[test]
fn prefixes_resolve_uniquely_and_report_missing_short_and_ambiguous_ones() {
    let _serial = serial();
    let scratch = seeded();
    let (first, second) = (
        "abababab-0000-4000-8000-000000000001",
        "abababab-1111-4111-8111-111111111111",
    );
    for id in [first, second] {
        assert_eq!(
            ok(
                &scratch.root,
                "add_task",
                &format!("{{\"spec\":{}}}", task(id, &[]))
            ),
            "{}\n"
        );
    }
    let result = lookup(
        &scratch.root,
        &[
            "aaaaaaaa",
            "bbbbbbbb",
            MISSING,
            "zz",
            "nomatch0123",
            "abababab",
            A,
        ],
    );
    assert_eq!(
        result.lookups,
        [
            Lookup::Found(tid(A)),
            Lookup::Found(tid(B)),
            Lookup::Missing,
            Lookup::InvalidPrefix,
            Lookup::Missing,
            Lookup::Ambiguous {
                candidates: vec![tid(first), tid(second)],
                truncated: false
            },
            Lookup::Found(tid(A)),
        ]
    );
    assert_eq!(ids(&result), [A, B, A]);
    assert!(
        !result.activity_complete,
        "unresolved inputs make activity unknown"
    );
}

#[test]
fn an_empty_initialized_store_has_complete_prefix_coverage() {
    let _serial = serial();
    let scratch = Scratch::new();
    assert_eq!(ok(&scratch.root, "initialize_empty", "{}"), "null\n");
    let result = lookup(&scratch.root, &["aaaaaaaa"]);
    assert_eq!(result.lookups, [Lookup::Missing]);
    let empty = page(&scratch.root, 10, false, None, None);
    assert_eq!(
        (ids(&empty), empty.count, empty.activity_complete),
        (vec![], Some(0), true)
    );
}

#[test]
fn a_damaged_task_has_no_header_and_is_not_listed_or_found() {
    let _serial = serial();
    let scratch = seeded();
    assert_eq!(
        ok(
            &scratch.root,
            "damage_task",
            &format!("{{\"task_id\":\"{B}\"}}")
        ),
        "null\n"
    );
    assert_eq!(
        ids(&page(&scratch.root, 100, false, None, None)),
        [E, D, C, A]
    );
    assert_eq!(
        lookup(&scratch.root, &[B, "bbbbbbbb"]).lookups,
        [Lookup::Missing, Lookup::Missing]
    );
}

#[test]
fn a_running_caller_without_a_decided_start_serves_an_uncertain_projection() {
    let _serial = serial();
    let scratch = seeded();
    let caller = format!(
        r#"{{"task_id":"{MISSING}","pid":{},"identity":"unknown","status":"running","marker":null}}"#,
        std::process::id()
    );
    ok(&scratch.root, "add_live_caller", &caller);
    let result = lookup(&scratch.root, &[MISSING]);
    let status = String::from_utf8(result.rows[0].status.clone().unwrap()).unwrap();
    for fragment in [
        r#""status":"running""#,
        r#""exit_code":null"#,
        r#""recovered":true"#,
        r#""needs_reconciliation":true"#,
        r#""process_identity_state":"uncertain""#,
    ] {
        assert!(status.contains(fragment), "{fragment} in {status}");
    }
}
