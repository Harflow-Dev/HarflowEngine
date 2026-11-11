#![cfg(unix)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

//! Launch-attempt and chain-stop writers: replacement, atomic refusal, clearing by a task
//! fixture, and their effect on the paging generation and the task-list watch revision.
mod common;
mod fixture_support;
use fixture_support::{A, B, World, done, later, quoted, tid, total, world};
use hf_storage::{ChainStop, Error, LaunchRow, TaskFixture};

const ROW: &str = "launch_attempts";

fn ids_in_order(w: &World) -> Vec<String> {
    let db = w.db();
    let mut statement = db
        .prepare("SELECT name FROM launch_attempts ORDER BY rowid")
        .unwrap();
    statement
        .query_map([], |r| r.get::<_, String>(0))
        .unwrap()
        .map(Result::unwrap)
        .collect()
}

fn empty_fixture() -> TaskFixture {
    TaskFixture {
        rows: Vec::new(),
        launches: Vec::new(),
        coverage: hf_domain::task_read::Coverage {
            discovery_complete: true,
            ownership_complete: true,
            source_complete: true,
        },
    }
}

#[test]
fn replacing_the_task_fixture_clears_both_new_tables() {
    let w = world();
    w.store
        .put_launch_attempt(&LaunchRow::waiting_for_resources(tid(A)), later(5))
        .unwrap();
    w.store
        .put_chain_stop(&tid(B), &ChainStop::Fault, later(5))
        .unwrap();
    assert_eq!((total(&w.db(), ROW), total(&w.db(), "chain_stops")), (1, 1));
    w.store
        .replace_task_fixture(&empty_fixture(), later(5))
        .unwrap();
    assert_eq!((total(&w.db(), ROW), total(&w.db(), "chain_stops")), (0, 0));
    let mut with_launch = empty_fixture();
    with_launch.launches = vec![LaunchRow::waiting_for_resources(tid(A))];
    w.store
        .replace_task_fixture(&with_launch, later(5))
        .unwrap();
    assert_eq!(total(&w.db(), ROW), 1);
}

#[test]
fn a_duplicate_launch_name_in_a_fixture_refuses_atomically() {
    let w = world();
    w.store
        .put_launch_attempt(&LaunchRow::waiting_for_resources(tid(A)), later(5))
        .unwrap();
    let row = LaunchRow::waiting_for_resources(tid(B));
    let error = w
        .store
        .replace_launch_fixture(&[row.clone(), row.clone()], later(5))
        .unwrap_err();
    assert!(matches!(error, Error::Incompatible), "{error:?}");
    assert_eq!(ids_in_order(&w), [A], "the earlier rows are untouched");
    let mut both = empty_fixture();
    both.launches = vec![row.clone(), row];
    let error = w.store.replace_task_fixture(&both, later(5)).unwrap_err();
    assert!(matches!(error, Error::Incompatible), "{error:?}");
}

#[test]
fn a_launch_may_share_an_id_with_an_ordinary_record() {
    let w = world();
    w.add(&done(A));
    w.store
        .put_launch_attempt(&LaunchRow::waiting_for_resources(tid(A)), later(5))
        .unwrap();
    w.store
        .replace_launch_fixture(&[LaunchRow::waiting_for_resources(tid(A))], later(5))
        .unwrap();
    assert_eq!(total(&w.db(), ROW), 1);
    assert_eq!(total(&w.db(), "task_facts"), 1);
}

#[test]
fn putting_a_launch_with_an_existing_name_replaces_it_in_place() {
    let w = world();
    let first = LaunchRow::waiting_for_resources(tid(A));
    let second = LaunchRow::waiting_for_resources(tid(B));
    w.store.put_launch_attempt(&first, later(5)).unwrap();
    w.store.put_launch_attempt(&second, later(5)).unwrap();
    let before = ids_in_order(&w);
    let mut changed = first;
    changed.state = Some("granted".into());
    changed.title = Some("new title".into());
    changed.resource_names = Some(vec!["a".into(), "b".into()]);
    w.store.put_launch_attempt(&changed, later(5)).unwrap();
    assert_eq!(total(&w.db(), ROW), 2);
    let mut names = ids_in_order(&w);
    names.sort();
    let mut expected = before;
    expected.sort();
    assert_eq!(names, expected, "the set of names is unchanged");
    assert_eq!(
        quoted(&w.db(), ROW, "state,title,resource_names", "name", A),
        "'granted'|'new title'|'[\"a\",\"b\"]'"
    );
}

#[test]
fn resource_names_are_stored_as_a_json_array_with_escapes() {
    let w = world();
    let mut row = LaunchRow::waiting_for_resources(tid(A));
    row.resource_names = Some(vec!["q\"uote".into(), "back\\slash".into(), "tab\t".into()]);
    w.store.put_launch_attempt(&row, later(5)).unwrap();
    let stored: String =
        fixture_support::one(&w.db(), "SELECT resource_names FROM launch_attempts", []);
    assert_eq!(stored, "[\"q\\\"uote\",\"back\\\\slash\",\"tab\\u0009\"]");
}

#[test]
fn launch_and_chain_stop_writes_leave_the_watch_revision_alone() {
    let w = world();
    let watch = w.watch();
    let generation = w.generation();
    w.store
        .put_chain_stop(
            &tid(A),
            &ChainStop::Marker {
                cancelled_task_id: Some("junk!".into()),
            },
            later(5),
        )
        .unwrap();
    w.store.remove_chain_stop(&tid(A), later(5)).unwrap();
    w.store
        .put_chain_stop(&tid(A), &ChainStop::Fault, later(5))
        .unwrap();
    assert_eq!(
        quoted(&w.db(), "chain_stops", "kind,cancelled_task_id", "root", A),
        "'fault'|NULL",
        "a removed marker can be written again"
    );
    assert_eq!(
        (w.watch(), w.generation()),
        (watch, generation.clone()),
        "chain stops touch neither the watch nor the paging generation"
    );
    w.store.remove_chain_stop(&tid(A), later(5)).unwrap();
    w.store.remove_chain_stop(&tid(A), later(5)).unwrap();
    assert_eq!(total(&w.db(), "chain_stops"), 0);

    w.store
        .put_launch_attempt(&LaunchRow::waiting_for_resources(tid(A)), later(5))
        .unwrap();
    assert_eq!(
        w.watch(),
        watch,
        "a launch write does not change the metadata revision"
    );
    assert_ne!(
        w.generation(),
        generation,
        "it advances the paging generation"
    );
    let generation = w.generation();
    w.store
        .replace_launch_fixture(&[LaunchRow::waiting_for_resources(tid(B))], later(5))
        .unwrap();
    assert_eq!(w.watch(), watch);
    assert_ne!(w.generation(), generation);
}

#[test]
fn a_marker_keeps_the_raw_attribution_including_oversize_text() {
    let w = world();
    let oversize = format!("{A}{}", "x".repeat(4096));
    w.store
        .put_chain_stop(
            &tid(A),
            &ChainStop::Marker {
                cancelled_task_id: Some(oversize.clone()),
            },
            later(5),
        )
        .unwrap();
    let stored: String = fixture_support::one(
        &w.db(),
        "SELECT cancelled_task_id FROM chain_stops WHERE root=?1",
        [A],
    );
    assert_eq!(stored, oversize);
    w.store
        .put_chain_stop(
            &tid(B),
            &ChainStop::Marker {
                cancelled_task_id: None,
            },
            later(5),
        )
        .unwrap();
    assert_eq!(
        quoted(&w.db(), "chain_stops", "kind,cancelled_task_id", "root", B),
        "'marker'|NULL"
    );
}

#[test]
fn the_default_chain_root_lookup_sees_only_valid_records_with_a_stored_root() {
    let w = world();
    let mut linked = done(B);
    linked.facts.chain_root = Some("not an id".into());
    w.add(&linked);
    w.add(&done(A));
    assert_eq!(
        w.store.fixture_chain_root(&tid(B), later(5)).unwrap(),
        Some(Some("not an id".into()))
    );
    assert_eq!(
        w.store.fixture_chain_root(&tid(A), later(5)).unwrap(),
        Some(None)
    );
    let missing = tid("99999999-9999-4999-8999-999999999999");
    assert_eq!(
        w.store.fixture_chain_root(&missing, later(5)).unwrap(),
        None
    );
}

fn oversized(id: &str) -> LaunchRow {
    let mut row = LaunchRow::waiting_for_resources(tid(id));
    row.title = Some("t".repeat(2 * 1024 * 1024));
    row
}

#[test]
fn an_oversized_launch_refuses_atomically_in_every_writer() {
    let w = world();
    w.store
        .put_launch_attempt(&LaunchRow::waiting_for_resources(tid(A)), later(5))
        .unwrap();
    let state = |w: &World| -> (String, i64, Vec<String>) {
        let (generation, revision) = w
            .db()
            .query_row(
                "SELECT d.generation,m.revision FROM task_dataset d,task_watch_meta m",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        (generation, revision, ids_in_order(w))
    };
    let before = state(&w);
    let put = w.store.put_launch_attempt(&oversized(B), later(5));
    let replace = w.store.replace_launch_fixture(&[oversized(B)], later(5));
    let mut fixture = empty_fixture();
    fixture.launches = vec![oversized(B)];
    let task_fixture = w.store.replace_task_fixture(&fixture, later(5));
    for result in [put, replace, task_fixture] {
        assert!(matches!(result.unwrap_err(), Error::LimitExceeded));
    }
    assert_eq!(
        state(&w),
        before,
        "rows, generation and revision are unchanged"
    );
}

#[test]
fn a_second_chain_stop_for_the_same_root_is_refused_until_removed() {
    let w = world();
    w.store
        .put_chain_stop(&tid(A), &ChainStop::Fault, later(5))
        .unwrap();
    assert!(
        w.store
            .put_chain_stop(
                &tid(A),
                &ChainStop::Marker {
                    cancelled_task_id: None
                },
                later(5)
            )
            .is_err()
    );
    w.store.remove_chain_stop(&tid(A), later(5)).unwrap();
    w.store.remove_chain_stop(&tid(A), later(5)).unwrap();
    w.store
        .put_chain_stop(
            &tid(A),
            &ChainStop::Marker {
                cancelled_task_id: None,
            },
            later(5),
        )
        .unwrap();
}
