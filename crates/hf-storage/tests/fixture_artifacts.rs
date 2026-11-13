#![cfg(unix)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

//! The artifact manifest writer.
mod common;
mod fixture_support;
use fixture_support::{A, B, C, World, count, done, is_hex32, later, one, quoted, tid, world};
use hf_domain::ctl::history::{PlanDirectory, PlanEvidence};
use hf_storage::{Error, FixtureArtifact, FixturePlan};

fn artifact(id: &str, path: &str) -> FixtureArtifact {
    FixtureArtifact {
        artifact_id: id.into(),
        path: path.into(),
        kind: "report".into(),
        note: "n".into(),
        registered_at: "2026-01-10T10:04:00+00:00".into(),
        provenance_method: "verified_process_lineage".into(),
        plan: None,
        encoded_len: 0,
        device: 1,
        inode: 2,
        size: 3,
        mtime_ns: 4,
    }
}

const ITEM: &str =
    "artifact_id,path,kind,note,registered_at,provenance_method,device,inode,size,mtime_ns";

fn items(w: &World, id: &str) -> Vec<String> {
    let db = w.db();
    let mut statement = db
        .prepare(&format!(
            "SELECT quote(ordinal)||'|'||{} FROM artifact_items WHERE task=?1 ORDER BY ordinal",
            ITEM.split(',')
                .map(|c| format!("quote({c})"))
                .collect::<Vec<_>>()
                .join("||'|'||")
        ))
        .unwrap();
    statement
        .query_map([id], |r| r.get::<_, String>(0))
        .unwrap()
        .map(Result::unwrap)
        .collect()
}

#[test]
fn artifacts_are_stored_in_order_with_unsigned_identity_as_decimal_text() {
    let w = world();
    w.add(&done(A));
    let mut huge = artifact("art-2", "/workspace/repo/out/b.txt");
    huge.device = u64::MAX;
    huge.inode = 9_223_372_036_854_775_808;
    huge.size = i64::MAX as u64;
    huge.mtime_ns = 18_446_744_073_709_551_615;
    w.store
        .replace_artifacts_fixture(
            &tid(A),
            true,
            &[artifact("art-1", "/workspace/repo/out/a.txt"), huge],
            later(5),
        )
        .unwrap();
    assert_eq!(
        items(&w, A),
        [
            "0|'art-1'|'/workspace/repo/out/a.txt'|'report'|'n'|'2026-01-10T10:04:00+00:00'|'verified_process_lineage'|'1'|'2'|3|'4'",
            "1|'art-2'|'/workspace/repo/out/b.txt'|'report'|'n'|'2026-01-10T10:04:00+00:00'|'verified_process_lineage'|'18446744073709551615'|'9223372036854775808'|9223372036854775807|'18446744073709551615'",
        ]
    );
}

#[test]
fn replacing_a_manifest_removes_old_items_and_changes_the_revision_every_time() {
    let w = world();
    w.add(&done(A));
    w.add(&done(B));
    let both = [artifact("a1", "/p/a"), artifact("a2", "/p/b")];
    w.store
        .replace_artifacts_fixture(&tid(B), true, &[artifact("keep", "/p/k")], later(5))
        .unwrap();
    w.store
        .replace_artifacts_fixture(&tid(A), true, &both, later(5))
        .unwrap();
    let r1: String = one(
        &w.db(),
        "SELECT revision FROM artifact_manifests WHERE task=?1",
        [A],
    );
    w.store
        .replace_artifacts_fixture(&tid(A), true, &both, later(5))
        .unwrap();
    let r2: String = one(
        &w.db(),
        "SELECT revision FROM artifact_manifests WHERE task=?1",
        [A],
    );
    assert!(is_hex32(&r1) && is_hex32(&r2));
    assert_ne!(r1, r2, "a rewrite of identical content is a new revision");
    w.store
        .replace_artifacts_fixture(&tid(A), true, &[artifact("only", "/p/z")], later(5))
        .unwrap();
    assert_eq!(items(&w, A).len(), 1);
    assert!(items(&w, A)[0].starts_with("0|'only'|"));
    assert_eq!(
        items(&w, B).len(),
        1,
        "another task's manifest is untouched"
    );
}

#[test]
fn an_invalid_header_manifest_is_stored_with_its_flag_and_no_items() {
    let w = world();
    w.add(&done(A));
    w.store
        .replace_artifacts_fixture(&tid(A), true, &[artifact("a1", "/p/a")], later(5))
        .unwrap();
    w.store
        .replace_artifacts_fixture(&tid(A), false, &[], later(5))
        .unwrap();
    assert_eq!(
        quoted(&w.db(), "artifact_manifests", "header_valid", "task", A),
        "0"
    );
    assert!(items(&w, A).is_empty());
}

#[test]
fn artifacts_need_a_valid_task_and_bounded_values() {
    let w = world();
    w.store.damage_task_fixture(&tid(B), later(5)).unwrap();
    w.add(&done(A));
    for id in [B, C] {
        let error = w
            .store
            .replace_artifacts_fixture(&tid(id), true, &[], later(5))
            .unwrap_err();
        assert!(matches!(error, Error::FixtureMissing), "{id}: {error:?}");
        assert_eq!(count(&w.db(), "artifact_manifests", "task", id), 0);
    }
    let mut too_big = artifact("a", "/p");
    too_big.size = i64::MAX as u64 + 1;
    let error = w
        .store
        .replace_artifacts_fixture(&tid(A), true, &[too_big], later(5))
        .unwrap_err();
    assert!(matches!(error, Error::LimitExceeded), "{error:?}");
    let many: Vec<FixtureArtifact> = (0..129).map(|n| artifact(&format!("a{n}"), "/p")).collect();
    let error = w
        .store
        .replace_artifacts_fixture(&tid(A), true, &many, later(5))
        .unwrap_err();
    assert!(matches!(error, Error::LimitExceeded), "{error:?}");
    assert_eq!(count(&w.db(), "artifact_manifests", "task", A), 0);
}

const PLAN: &str = "observed_backend_plan";

fn planned(id: &str, plan: Option<FixturePlan>) -> FixtureArtifact {
    let mut item = artifact(id, "/p/plan");
    item.provenance_method = PLAN.into();
    item.plan = plan;
    item
}

fn evidence(directory: Option<(&str, u64, u64)>) -> Option<FixturePlan> {
    Some(FixturePlan {
        tool_use_id: "toolu_01".into(),
        directory: directory.map(|(path, device, inode)| (path.to_string(), device, inode)),
    })
}

#[test]
fn plan_evidence_is_read_back_exactly_and_is_absent_for_other_methods() {
    let w = world();
    w.add(&done(A));
    let items = [
        planned(
            "p1",
            evidence(Some(("/plans/a", u64::MAX, 9_223_372_036_854_775_808))),
        ),
        planned("p2", evidence(None)),
        artifact("o1", "/p/o"),
    ];
    w.store
        .replace_artifacts_fixture(&tid(A), true, &items, later(5))
        .unwrap();
    let found = w
        .session()
        .snapshot(later(30), |s| s.artifact_items(&tid(A), 0, 10))
        .unwrap();
    let plans: Vec<_> = found.iter().map(|i| i.plan.clone()).collect();
    assert_eq!(
        plans,
        [
            Some(PlanEvidence {
                tool_use_id: "toolu_01".into(),
                directory: Some(PlanDirectory {
                    path: "/plans/a".into(),
                    device: u64::MAX,
                    inode: 9_223_372_036_854_775_808,
                }),
            }),
            Some(PlanEvidence {
                tool_use_id: "toolu_01".into(),
                directory: None,
            }),
            None,
        ]
    );
    let stored: String = one(
        &w.db(),
        "SELECT plan_device||'|'||plan_inode FROM artifact_items WHERE artifact_id='p1'",
        [],
    );
    assert_eq!(stored, "18446744073709551615|9223372036854775808");
}

#[test]
fn the_writer_refuses_plan_evidence_that_does_not_match_the_method() {
    let w = world();
    w.add(&done(A));
    let mut stray = artifact("s1", "/p/s");
    stray.plan = evidence(None);
    for items in [vec![stray], vec![planned("s2", None)]] {
        let error = w
            .store
            .replace_artifacts_fixture(&tid(A), true, &items, later(5))
            .unwrap_err();
        assert!(
            matches!(&error, Error::Sqlite(rusqlite::Error::SqliteFailure(failure, _))
                if failure.code == rusqlite::ErrorCode::ConstraintViolation),
            "{error:?}"
        );
        assert_eq!(count(&w.db(), "artifact_items", "task", A), 0);
    }
}

fn insert(w: &World, method: &str, tool: &str, directory: [&str; 3]) -> bool {
    let raw = w.raw();
    let revision = "0123456789abcdef0123456789abcdef";
    raw.execute_batch(&format!(
        "INSERT OR IGNORE INTO artifact_manifests VALUES('m',1,'{revision}')"
    ))
    .unwrap();
    raw.execute(
        "INSERT INTO artifact_items VALUES('m',(SELECT count(*) FROM artifact_items),'id','/p','k','n','t',?1,'1','2',3,'4',?2,?3,?4,?5,0)",
        rusqlite::params![
            method,
            (tool != "-").then_some(tool),
            (directory[0] != "-").then_some(directory[0]),
            (directory[1] != "-").then_some(directory[1]),
            (directory[2] != "-").then_some(directory[2]),
        ],
    )
    .is_ok()
}

fn at(path: &str) -> [&str; 3] {
    [path, "1", "2"]
}

#[test]
fn the_item_table_enforces_the_plan_evidence_invariants() {
    let w = world();
    let none = ["-", "-", "-"];
    assert!(insert(&w, PLAN, "t", none));
    assert!(insert(&w, PLAN, "t", at("/p")));
    assert!(insert(&w, "engine_output", "-", none));
    // The tool-use id is present exactly for an observed plan.
    assert!(!insert(&w, PLAN, "-", none));
    assert!(!insert(&w, "engine_output", "t", none));
    assert!(!insert(&w, "engine_output", "t", at("/p")));
    assert!(!insert(&w, "engine_output", "-", at("/p")));
    // One to 256 bytes.
    assert!(!insert(&w, PLAN, "", none));
    assert!(insert(&w, PLAN, &"x".repeat(256), none));
    assert!(!insert(&w, PLAN, &"x".repeat(257), none));
    assert!(!insert(&w, PLAN, &"\u{e9}".repeat(129), none));
    // The directory columns are all present or all absent, one to 4096 bytes.
    assert!(!insert(&w, PLAN, "t", ["/p", "-", "-"]));
    assert!(!insert(&w, PLAN, "t", ["/p", "1", "-"]));
    assert!(!insert(&w, PLAN, "t", ["-", "1", "2"]));
    assert!(!insert(&w, PLAN, "t", ["/p", "-", "2"]));
    assert!(!insert(&w, PLAN, "t", at("")));
    assert!(insert(&w, PLAN, "t", at(&format!("/{}", "d".repeat(4095)))));
    assert!(!insert(
        &w,
        PLAN,
        "t",
        at(&format!("/{}", "d".repeat(4096)))
    ));
}

#[test]
fn a_plan_identity_that_is_not_unsigned_decimal_text_is_a_malformed_row() {
    let w = world();
    w.add(&done(A));
    let item = planned("p1", evidence(Some(("/plans/a", 1, 2))));
    w.store
        .replace_artifacts_fixture(&tid(A), true, &[item], later(5))
        .unwrap();
    // Text that is not decimal digits cannot be stored at all.
    for bad in ["-1", "abc", ""] {
        let refused = w
            .raw()
            .execute("UPDATE artifact_items SET plan_inode=?1", [bad]);
        assert!(refused.is_err(), "{bad}");
    }
    // Digits beyond u64 are stored but do not decode.
    w.raw()
        .execute(
            "UPDATE artifact_items SET plan_inode='18446744073709551616'",
            [],
        )
        .unwrap();
    let error = w
        .session()
        .snapshot(later(30), |s| s.artifact_items(&tid(A), 0, 10))
        .unwrap_err();
    assert!(matches!(error, Error::MalformedRow), "{error:?}");
}

#[test]
fn the_longest_plan_evidence_on_one_item_reads_back_in_a_page() {
    let w = world();
    w.add(&done(A));
    let directory = format!("/{}", "d".repeat(4095));
    let mut item = planned("p1", evidence(Some((&directory, 1, 2))));
    item.plan.as_mut().unwrap().tool_use_id = "t".repeat(256);
    item.note = "n".repeat(4096);
    item.path = format!("/{}", "p".repeat(4095));
    w.store
        .replace_artifacts_fixture(
            &tid(A),
            true,
            &[item.clone(), artifact("o1", "/p/o")],
            later(5),
        )
        .unwrap();
    let found = w
        .session()
        .snapshot(later(30), |s| s.artifact_items(&tid(A), 0, 10))
        .unwrap();
    let plan = found[0].plan.clone().unwrap();
    assert_eq!(plan.tool_use_id.len(), 256);
    assert_eq!(plan.directory.unwrap().path.len(), 4096);
    assert_eq!(found.len(), 2);
}
