#![cfg(unix)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

//! The history reads of the ctl snapshot: valid-task joins, prompts and their ceiling,
//! manifests, items and continuation links.
mod common;
mod fixture_support;
use fixture_support::{A, B, C, World, done, later, task, tid, world};
use hf_domain::ctl::history::PromptBlob;
use hf_storage::{Error, FixtureArtifact, FixturePrompt};

/// sha256("abc"), a published test vector.
const ABC: &str = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";
const CEILING: usize = 4 * 1024 * 1024;

fn artifact(n: usize) -> FixtureArtifact {
    FixtureArtifact {
        artifact_id: format!("art-{n:03}"),
        path: format!("/workspace/repo/out/f{n:03}.txt"),
        kind: "report".into(),
        note: "n".into(),
        registered_at: "2026-01-10T10:04:00+00:00".into(),
        provenance_method: "verified_process_lineage".into(),
        plan: None,
        device: 1,
        inode: n as u64,
        size: 3,
        mtime_ns: 4,
    }
}

fn manifest(w: &World, id: &str, count: usize) {
    let items: Vec<FixtureArtifact> = (0..count).map(artifact).collect();
    w.store
        .replace_artifacts_fixture(&tid(id), true, &items, later(5))
        .unwrap();
}

fn sidecar(w: &World, id: &str, name: String, blob: Option<Vec<u8>>) {
    let mut record = task(id, "completed", Some(0), true);
    record.prompt = FixturePrompt {
        inline_text: None,
        preview: Some("preview".into()),
        claimed_sha256: None,
        source_name: Some(name),
        blob,
    };
    w.add(&record);
}

fn named(id: &str, digest: &str) -> String {
    format!("{id}.prompt.{digest}.txt")
}

fn with_snapshot<T>(
    w: &World,
    action: impl FnOnce(&hf_storage::TaskSnapshot<'_>) -> Result<T, Error>,
) -> Result<T, Error> {
    w.session().snapshot(later(30), action)
}

#[test]
fn a_damaged_task_has_no_prompt_manifest_items_or_link() {
    let w = world();
    w.add(&done(A));
    w.add(&done(B));
    manifest(&w, A, 2);
    w.store.damage_task_fixture(&tid(A), later(5)).unwrap();
    with_snapshot(&w, |s| {
        assert_eq!(s.prompt(&tid(A), 100)?, None);
        assert_eq!(s.artifact_manifest(&tid(A))?, None);
        assert_eq!(s.artifact_items(&tid(A), 0, 10)?, Vec::new());
        assert_eq!(s.continuation_link(&tid(A))?, None);
        assert!(s.prompt(&tid(B), 100)?.is_some());
        Ok(())
    })
    .unwrap();
}

#[test]
fn an_unknown_task_has_nothing_either() {
    let w = world();
    with_snapshot(&w, |s| {
        assert_eq!(s.prompt(&tid(C), 100)?, None);
        assert_eq!(s.artifact_manifest(&tid(C))?, None);
        assert_eq!(s.continuation_link(&tid(C))?, None);
        Ok(())
    })
    .unwrap();
}

#[test]
fn the_manifest_counts_at_most_one_hundred_and_one_and_each_write_has_its_own_revision() {
    let w = world();
    w.add(&done(A));
    for (count, expected) in [(0, 0), (3, 3), (100, 100), (101, 101), (128, 101)] {
        manifest(&w, A, count);
        let head = with_snapshot(&w, |s| s.artifact_manifest(&tid(A)))
            .unwrap()
            .unwrap();
        assert_eq!((head.count, head.header_valid), (expected, true), "{count}");
        assert_eq!(head.readable(), expected <= 100);
    }
    let first = with_snapshot(&w, |s| s.artifact_manifest(&tid(A)))
        .unwrap()
        .unwrap()
        .revision;
    manifest(&w, A, 128);
    let second = with_snapshot(&w, |s| s.artifact_manifest(&tid(A)))
        .unwrap()
        .unwrap()
        .revision;
    assert_ne!(first, second, "an identical rewrite is a new revision");
    assert_eq!(first.len(), 32);
}

#[test]
fn an_invalid_header_flag_is_carried_and_makes_the_manifest_unreadable() {
    let w = world();
    w.add(&done(A));
    w.store
        .replace_artifacts_fixture(&tid(A), false, &[artifact(0)], later(5))
        .unwrap();
    let head = with_snapshot(&w, |s| s.artifact_manifest(&tid(A)))
        .unwrap()
        .unwrap();
    assert!(!head.header_valid && !head.readable());
}

#[test]
fn items_come_in_ordinal_order_by_offset_and_limit_with_unsigned_identity() {
    let w = world();
    w.add(&done(A));
    let mut huge = artifact(2);
    huge.device = u64::MAX;
    huge.inode = 9_223_372_036_854_775_808;
    w.store
        .replace_artifacts_fixture(
            &tid(A),
            true,
            &[artifact(0), artifact(1), huge, artifact(3)],
            later(5),
        )
        .unwrap();
    let ids = |offset, limit| -> Vec<String> {
        with_snapshot(&w, |s| s.artifact_items(&tid(A), offset, limit))
            .unwrap()
            .into_iter()
            .map(|item| item.artifact_id)
            .collect()
    };
    assert_eq!(ids(0, 10), ["art-000", "art-001", "art-002", "art-003"]);
    assert_eq!(ids(1, 2), ["art-001", "art-002"]);
    assert_eq!(ids(3, 5), ["art-003"]);
    assert!(ids(4, 5).is_empty());
    let items = with_snapshot(&w, |s| s.artifact_items(&tid(A), 2, 1)).unwrap();
    assert_eq!(
        (items[0].registered.device, items[0].registered.inode),
        (u64::MAX, 9_223_372_036_854_775_808)
    );
    assert_eq!(
        (items[0].method.as_str(), items[0].kind.as_str()),
        ("verified_process_lineage", "report")
    );
}

#[test]
fn an_observation_that_does_not_parse_is_a_malformed_row() {
    let w = world();
    w.add(&done(A));
    manifest(&w, A, 1);
    for (column, value) in [
        ("device", "x"),
        ("inode", "-1"),
        ("mtime_ns", "1.5"),
        ("mtime_ns", "9223372036854775808"),
    ] {
        w.raw()
            .execute(
                &format!("UPDATE artifact_items SET {column}=?1 WHERE task=?2"),
                [value, A],
            )
            .unwrap();
        let outcome = with_snapshot(&w, |s| s.artifact_items(&tid(A), 0, 10));
        assert!(
            matches!(outcome, Err(Error::MalformedRow)),
            "{column}={value}: {outcome:?}"
        );
        manifest(&w, A, 1);
    }
    w.raw()
        .execute("UPDATE artifact_items SET mtime_ns='-5' WHERE task=?1", [A])
        .unwrap();
    assert_eq!(
        with_snapshot(&w, |s| s.artifact_items(&tid(A), 0, 1)).unwrap()[0]
            .registered
            .mtime_ns,
        -5
    );
}

#[test]
fn the_continuation_link_distinguishes_a_missing_task_from_one_that_continues_nothing() {
    let w = world();
    let mut child = done(B);
    child.facts.continuation_of = Some(tid(A));
    w.add(&done(A));
    w.add(&child);
    with_snapshot(&w, |s| {
        assert_eq!(s.continuation_link(&tid(A))?, Some(None));
        assert_eq!(s.continuation_link(&tid(B))?, Some(Some(tid(A))));
        assert_eq!(s.continuation_link(&tid(C))?, None);
        Ok(())
    })
    .unwrap();
}

#[test]
fn an_inline_prompt_is_served_whole_and_unbound() {
    let w = world();
    w.add(&done(A));
    let stored = with_snapshot(&w, |s| s.prompt(&tid(A), 10))
        .unwrap()
        .unwrap();
    assert_eq!(stored.inline.as_deref(), Some("Summarize the repo."));
    assert!(!stored.bound);
    assert_eq!(stored.blob, PromptBlob::Absent);
    assert_eq!(stored.title.as_deref(), Some("First task"));
}

#[test]
fn a_sidecar_is_classified_by_its_name_and_its_blob() {
    let w = world();
    sidecar(&w, A, "wrong-name.txt".into(), Some(b"abc".to_vec()));
    sidecar(&w, B, named(B, ABC), Some(b"abc".to_vec()));
    sidecar(&w, C, named(C, &"0".repeat(64)), Some(b"abc".to_vec()));
    let read = |id| {
        with_snapshot(&w, |s| s.prompt(&tid(id), 10))
            .unwrap()
            .unwrap()
    };
    let foreign = read(A);
    assert!(
        (!foreign.bound) && foreign.blob == PromptBlob::Absent,
        "a foreign name is not bound and its blob is not read"
    );
    let verified = read(B);
    assert!(verified.bound);
    assert_eq!(verified.blob, PromptBlob::Verified(b"abc".to_vec()));
    assert_eq!(verified.preview.as_deref(), Some("preview"));
    assert_eq!(read(C).blob, PromptBlob::Mismatch);
}

#[test]
fn a_sidecar_without_a_blob_is_bound_and_absent() {
    let w = world();
    sidecar(&w, A, named(A, ABC), None);
    let stored = with_snapshot(&w, |s| s.prompt(&tid(A), 10))
        .unwrap()
        .unwrap();
    assert!(stored.bound);
    assert_eq!(stored.blob, PromptBlob::Absent);
}

#[test]
fn names_that_are_not_exactly_task_prompt_digest_txt_are_not_bound() {
    let upper = ABC.to_uppercase();
    for name in [
        format!("{A}.prompt.{upper}.txt"),
        format!("{A}.prompt.{}.txt", &ABC[..63]),
        format!("{A}.prompt.{ABC}0.txt"),
        format!("{A}.prompt.{ABC}.txt.bak"),
        format!("{B}.prompt.{ABC}.txt"),
        format!("{A}-prompt-{ABC}.txt"),
    ] {
        let fresh = world();
        sidecar(&fresh, A, name.clone(), Some(b"abc".to_vec()));
        let stored = with_snapshot(&fresh, |s| s.prompt(&tid(A), 10))
            .unwrap()
            .unwrap();
        assert!(!stored.bound, "{name}");
    }
}

fn big_sidecar(size: usize, digest: &str) -> World {
    let w = world();
    sidecar(&w, A, named(A, digest), Some(b"abc".to_vec()));
    let blob: Vec<u8> = (0..size).map(|n| b'a' + (n % 26) as u8).collect();
    w.raw()
        .execute(
            "UPDATE task_prompts SET blob=?1 WHERE task=?2",
            rusqlite::params![blob, A],
        )
        .unwrap();
    w
}

#[test]
fn a_blob_over_the_ceiling_is_a_bounded_unverified_prefix() {
    let w = big_sidecar(CEILING + 1, ABC);
    for max in [1usize, 10, 4096] {
        let stored = with_snapshot(&w, |s| s.prompt(&tid(A), max))
            .unwrap()
            .unwrap();
        let PromptBlob::Unverified { prefix, total } = stored.blob else {
            panic!("{:?}", stored.blob)
        };
        assert_eq!(total, CEILING as u64 + 1);
        assert_eq!(prefix.len(), max + 4);
        assert_eq!(prefix[0], b'a');
    }
}

#[test]
fn a_blob_at_the_ceiling_is_read_whole_and_hashed() {
    let w = big_sidecar(CEILING, ABC);
    let stored = with_snapshot(&w, |s| s.prompt(&tid(A), 10))
        .unwrap()
        .unwrap();
    assert_eq!(
        stored.blob,
        PromptBlob::Mismatch,
        "the digest of another body does not match"
    );
}

#[test]
fn the_text_of_a_prompt_row_counts_against_the_session_budget_but_the_blob_does_not() {
    let w = world();
    let mut record = task(A, "completed", Some(0), true);
    record.prompt.inline_text = Some("x".repeat(200));
    w.add(&record);
    w.raw()
        .execute(
            "UPDATE task_prompts SET inline_text=?1 WHERE task=?2",
            rusqlite::params!["x".repeat(2_000_000), A],
        )
        .unwrap();
    let outcome = with_snapshot(&w, |s| s.prompt(&tid(A), 10));
    assert!(matches!(outcome, Err(Error::LimitExceeded)), "{outcome:?}");
}

#[test]
fn a_row_of_a_wrong_shape_is_malformed() {
    let w = world();
    sidecar(&w, A, named(A, ABC), Some(b"abc".to_vec()));
    w.raw()
        .execute("UPDATE task_prompts SET blob='text' WHERE task=?1", [A])
        .unwrap();
    let outcome = with_snapshot(&w, |s| s.prompt(&tid(A), 10));
    assert!(matches!(outcome, Err(Error::MalformedRow)), "{outcome:?}");
}
