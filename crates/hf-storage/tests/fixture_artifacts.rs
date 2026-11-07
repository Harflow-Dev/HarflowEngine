#![cfg(unix)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

//! The artifact manifest writer.
mod common;
mod fixture_support;
use fixture_support::{A, B, C, World, count, done, is_hex32, later, one, quoted, tid, world};
use hf_storage::{Error, FixtureArtifact};

fn artifact(id: &str, path: &str) -> FixtureArtifact {
    FixtureArtifact {
        artifact_id: id.into(),
        path: path.into(),
        kind: "report".into(),
        note: "n".into(),
        registered_at: "2026-01-10T10:04:00+00:00".into(),
        provenance_method: "verified_process_lineage".into(),
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
