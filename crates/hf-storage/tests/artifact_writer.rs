#![cfg(unix)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

//! `Store::register_artifact`: what an added item stores and publishes, ordinal assignment,
//! the duplicate rules of both registrants, and the task and manifest admission checks.
mod artifact_writer_support;
mod common;
mod fixture_support;
mod task_writer_support;
use artifact_writer_support::{
    added, engine_item, item, items, lengths, manifest, observation, plan_item, refused, register,
    revision, state, usual,
};
use fixture_support::{A, B, C, World, artifact_item, is_hex32, later, tid, world};
use hf_storage::{Error, Registered, Registrant};
use task_writer_support::{done, published, put, running};

const CALLER: Registrant = Registrant::Caller;
const ENGINE: Registrant = Registrant::Engine;

#[test]
fn a_first_registration_creates_a_valid_manifest_and_stores_every_column() {
    let w = world();
    put(&w, &running(A));
    let before = published(&w);
    added(
        &w,
        A,
        &item("art-1", "/workspace/repo/out/a.txt"),
        usual(),
        CALLER,
    );
    let mut plan = plan_item(
        "p-1",
        Some(("/plans/a", u64::MAX, 9_223_372_036_854_775_808)),
    );
    plan.registered.mtime_ns = i64::MIN;
    added(&w, A, &plan, lengths(164, 700), ENGINE);
    let (header, rev) = manifest(&w, A)
        .split_once('|')
        .map(|(h, r)| (h.to_owned(), r.to_owned()))
        .unwrap();
    assert_eq!(header, "1");
    assert!(is_hex32(&rev));
    assert_eq!(
        items(&w, A),
        [
            "0|'art-1'|'/workspace/repo/out/a.txt'|'report'|'n'|'2026-01-10T10:04:00+00:00'|'verified_process_lineage'|'1'|'2'|3|'4'|NULL|NULL|NULL|NULL|373",
            "1|'p-1'|'/p/plan'|'report'|'n'|'2026-01-10T10:04:00+00:00'|'observed_backend_plan'|'1'|'2'|3|'-9223372036854775808'|'toolu_01'|'/plans/a'|'18446744073709551615'|'9223372036854775808'|700",
        ]
    );
    let after = published(&w);
    assert_eq!(
        (&after.0, after.1),
        (&before.0, before.1),
        "no task publication"
    );
    assert_eq!(after.2, before.2 + 2, "one commit per registration");
}

#[test]
fn each_addition_rotates_the_revision_and_advances_the_sequence_by_one() {
    let w = world();
    put(&w, &running(A));
    put(&w, &running(B));
    added(&w, B, &item("b", "/p/b"), usual(), CALLER);
    let other = (manifest(&w, B), items(&w, B));
    let mut seen = Vec::new();
    for n in 0..4u64 {
        let (generation, watch, sequence) = published(&w);
        let mut next = item(&format!("a{n}"), "/p/a");
        next.registered = observation(n);
        added(
            &w,
            A,
            &next,
            usual(),
            if n % 2 == 0 { CALLER } else { ENGINE },
        );
        assert_eq!(published(&w), (generation, watch, sequence + 1));
        let current = revision(&w, A);
        assert!(!seen.contains(&current), "a fresh revision every time");
        seen.push(current);
    }
    assert_eq!(
        (manifest(&w, B), items(&w, B)),
        other,
        "another task is untouched"
    );
}

#[test]
fn a_new_item_takes_the_ordinal_after_the_largest_stored_one() {
    let w = world();
    put(&w, &running(A));
    let seeded = [
        artifact_item("f0", "/f/0"),
        artifact_item("f1", "/f/1"),
        artifact_item("f2", "/f/2"),
    ];
    w.store
        .replace_artifacts_fixture(&tid(A), true, &seeded, later(5))
        .unwrap();
    w.raw()
        .execute(
            "DELETE FROM artifact_items WHERE task=?1 AND ordinal=1",
            [A],
        )
        .unwrap();
    added(&w, A, &item("n", "/p/n"), usual(), CALLER);
    let ordinals: Vec<String> = items(&w, A)
        .iter()
        .map(|row| row.split('|').take(2).collect::<Vec<_>>().join("|"))
        .collect();
    assert_eq!(ordinals, ["0|'f0'", "2|'f2'", "3|'n'"]);
    // A manifest with a valid header and no items starts at zero.
    put(&w, &running(B));
    w.store
        .replace_artifacts_fixture(&tid(B), true, &[], later(5))
        .unwrap();
    added(&w, B, &item("b", "/p/b"), usual(), CALLER);
    assert!(items(&w, B)[0].starts_with("0|'b'|"));
}

fn existing(
    w: &World,
    id: &str,
    request: &hf_domain::ctl::history::ArtifactItem,
    who: Registrant,
) -> hf_domain::ctl::history::ArtifactItem {
    let before = state(w, id);
    let Registered::Existing(found) = register(w, id, request, usual(), who).unwrap() else {
        panic!("{}: expected a duplicate", request.artifact_id);
    };
    assert_eq!(
        state(w, id),
        before,
        "a duplicate commits nothing and keeps the revision"
    );
    *found
}

#[test]
fn a_caller_duplicate_is_the_same_path_kind_note_and_observation() {
    let w = world();
    put(&w, &running(A));
    let first = item("first", "/p/a");
    added(&w, A, &first, usual(), CALLER);
    let mut again = item("second", "/p/a");
    again.registered_at = "2026-01-10T11:00:00+00:00".into();
    assert_eq!(
        existing(&w, A, &again, CALLER),
        first,
        "the stored item is returned"
    );
    let variants: [fn(&mut hf_domain::ctl::history::ArtifactItem); 7] = [
        |i| i.path = "/p/other".into(),
        |i| i.kind = "log".into(),
        |i| i.note = "other".into(),
        |i| i.registered.device = 9,
        |i| i.registered.inode = 9,
        |i| i.registered.size = 9,
        |i| i.registered.mtime_ns = 9,
    ];
    for (n, change) in variants.into_iter().enumerate() {
        let mut different = item(&format!("v{n}"), "/p/a");
        change(&mut different);
        added(&w, A, &different, usual(), CALLER);
    }
    assert_eq!(items(&w, A).len(), 8);
}

#[test]
fn an_engine_duplicate_ignores_kind_and_note_and_returns_the_stored_item() {
    let w = world();
    put(&w, &done(A));
    let mut stored = plan_item("p1", Some(("/plans/a", 7, 8)));
    stored.path = "/p/a".into();
    added(&w, A, &stored, usual(), ENGINE);
    let mut request = engine_item("e2", "/p/a");
    request.kind = "other".into();
    request.note = "different".into();
    assert_eq!(existing(&w, A, &request, ENGINE), stored);
    // A caller's rule is stricter: the same request is new to a running task's caller.
    put(&w, &running(B));
    added(&w, B, &item("c1", "/p/a"), usual(), CALLER);
    assert_eq!(existing(&w, B, &request, ENGINE).artifact_id, "c1");
    let mut changed = engine_item("e3", "/p/a");
    changed.kind = "other".into();
    added(&w, B, &changed, usual(), CALLER);
}

#[test]
fn a_duplicate_returns_the_first_equal_item_in_manifest_order() {
    let w = world();
    put(&w, &running(A));
    let twins = [
        artifact_item("x", "/p/x"),
        artifact_item("t1", "/p/t"),
        artifact_item("t2", "/p/t"),
    ];
    w.store
        .replace_artifacts_fixture(&tid(A), true, &twins, later(5))
        .unwrap();
    for who in [CALLER, ENGINE] {
        assert_eq!(existing(&w, A, &item("new", "/p/t"), who).artifact_id, "t1");
    }
}

#[test]
fn a_missing_damaged_or_header_less_task_is_refused() {
    let w = world();
    put(&w, &done(A));
    w.store.damage_task_fixture(&tid(A), later(5)).unwrap();
    put(&w, &running(B));
    w.raw()
        .execute("DELETE FROM task_headers WHERE id=?1", [B])
        .unwrap();
    for id in [A, C] {
        for who in [CALLER, ENGINE] {
            refused(
                &w,
                id,
                || register(&w, id, &item("a", "/p"), usual(), who),
                |e| matches!(e, Error::TaskMissing),
            );
        }
    }
    refused(
        &w,
        B,
        || register(&w, B, &item("a", "/p"), usual(), CALLER),
        |e| matches!(e, Error::TaskMissing),
    );
    added(&w, B, &item("a", "/p"), usual(), ENGINE);
}

#[test]
fn a_caller_is_refused_on_a_terminal_task_and_the_engine_is_not() {
    for status in ["completed", "failed", "timed_out", "cancelled"] {
        let w = world();
        put(&w, &task_writer_support::create(A, status, Some(0), "p"));
        refused(
            &w,
            A,
            || register(&w, A, &item("a", "/p"), usual(), CALLER),
            |e| matches!(e, Error::TaskFinished),
        );
        added(&w, A, &item("a", "/p"), usual(), ENGINE);
        // Even a duplicate of a stored item is refused to a caller once the task finished.
        refused(
            &w,
            A,
            || register(&w, A, &item("a", "/p"), usual(), CALLER),
            |e| matches!(e, Error::TaskFinished),
        );
    }
}

#[test]
fn the_header_recorded_status_decides_before_the_facts_status() {
    let w = world();
    put(&w, &running(A));
    put(&w, &done(B));
    let raw = w.raw();
    raw.execute(
        "UPDATE task_headers SET recorded_status='completed' WHERE id=?1",
        [A],
    )
    .unwrap();
    raw.execute(
        "UPDATE task_headers SET recorded_status='running' WHERE id=?1",
        [B],
    )
    .unwrap();
    refused(
        &w,
        A,
        || register(&w, A, &item("a", "/p"), usual(), CALLER),
        |e| matches!(e, Error::TaskFinished),
    );
    added(&w, B, &item("b", "/p"), usual(), CALLER);
    raw.execute(
        "UPDATE task_headers SET recorded_status=NULL WHERE id IN (?1,?2)",
        [A, B],
    )
    .unwrap();
    added(&w, A, &item("a", "/p"), usual(), CALLER);
    refused(
        &w,
        B,
        || register(&w, B, &item("c", "/q"), usual(), CALLER),
        |e| matches!(e, Error::TaskFinished),
    );
}

#[test]
fn a_manifest_with_an_invalid_header_is_incompatible_after_the_task_checks() {
    let w = world();
    put(&w, &running(A));
    put(&w, &done(B));
    for id in [A, B] {
        w.store
            .replace_artifacts_fixture(&tid(id), false, &[], later(5))
            .unwrap();
    }
    for who in [CALLER, ENGINE] {
        refused(
            &w,
            A,
            || register(&w, A, &item("a", "/p"), usual(), who),
            |e| matches!(e, Error::Incompatible),
        );
    }
    refused(
        &w,
        B,
        || register(&w, B, &item("a", "/p"), usual(), CALLER),
        |e| matches!(e, Error::TaskFinished),
    );
    refused(
        &w,
        B,
        || register(&w, B, &item("a", "/p"), usual(), ENGINE),
        |e| matches!(e, Error::Incompatible),
    );
}

#[test]
fn plan_evidence_that_does_not_match_the_method_is_refused_before_the_transaction() {
    let w = world();
    put(&w, &running(A));
    let mut stray = item("s", "/p/s");
    stray.plan = plan_item("x", None).plan;
    let mut bare = plan_item("b", None);
    bare.plan = None;
    // C has no task, so a LimitExceeded there wins over TaskMissing: the check runs before the lookup.
    for bad in [stray, bare] {
        for id in [A, C] {
            refused(
                &w,
                id,
                || register(&w, id, &bad, usual(), ENGINE),
                |e| matches!(e, Error::LimitExceeded),
            );
        }
    }
}
