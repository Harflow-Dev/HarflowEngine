#![cfg(unix)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

//! The launch rows of the pending reads: filename order and resumption, field mapping, bare
//! records, text that is not UTF-8, and the refusal seen by the ordinary lookup.
mod common;
mod fixture_support;
mod pending_support;
use fixture_support::{A, B, C, done, later, tid, world};
use hf_domain::task_read::Lookup;
use pending_support::{ids, invalid, launch, next_name, put, refused, rows};

#[test]
fn rows_come_in_filename_order_and_skip_invalid_names() {
    let w = world();
    put(&w, &launch(B, "reserved"));
    put(&w, &launch(A, "claimed"));
    put(&w, &invalid("zzz not an id"));
    put(&w, &launch(C, "granted"));
    assert_eq!(ids(&rows(&w, None, 10)), [A, B, C]);
    assert_eq!(ids(&rows(&w, None, 2)), [A, B]);
    assert_eq!(ids(&rows(&w, Some(A), 10)), [B, C]);
    assert_eq!(ids(&rows(&w, Some(C), 10)), Vec::<&str>::new());
    assert_eq!(ids(&rows(&w, None, 0)), Vec::<&str>::new());
}

#[test]
fn resumption_uses_the_full_filename_so_prefix_nested_ids_are_not_skipped() {
    let w = world();
    put(&w, &launch("ab", "reserved"));
    put(&w, &launch("ab-c", "reserved"));
    assert_eq!(ids(&rows(&w, None, 10)), ["ab-c", "ab"]);
    assert_eq!(ids(&rows(&w, Some("ab-c"), 10)), ["ab"]);
    assert_eq!(ids(&rows(&w, Some("ab"), 10)), Vec::<&str>::new());
}

#[test]
fn a_row_carries_its_columns() {
    let w = world();
    let mut row = launch(A, "waiting_for_resources");
    row.not_started_reason = Some("why".into());
    row.spawned_by = Some(B.into());
    row.root_task_id = Some(C.into());
    row.parent_task_id = Some("p".into());
    row.chain_root_task_id = Some("q".into());
    row.resource_names = Some(vec!["gpu".into(), "disk".into()]);
    row.has_resources = Some(true);
    row.has_worktree = Some(true);
    put(&w, &row);
    let found = rows(&w, None, 5).remove(0);
    assert_eq!(found.id, A);
    assert!(!found.record && found.text_valid);
    assert_eq!(found.state.as_deref(), Some("waiting_for_resources"));
    assert_eq!(found.refusal, None);
    assert_eq!(found.generation.as_deref(), Some("c".repeat(32).as_str()));
    assert_eq!(found.title.as_deref(), Some("Title"));
    assert_eq!(found.backend.as_deref(), Some("claude"));
    assert_eq!(found.repo_path.as_deref(), Some("/repo"));
    assert_eq!(found.not_started_reason.as_deref(), Some("why"));
    assert_eq!(found.spawned_by.as_deref(), Some(B));
    assert_eq!(found.root_task_id.as_deref(), Some(C));
    assert_eq!(found.parent_task_id.as_deref(), Some("p"));
    assert_eq!(found.chain_root_task_id.as_deref(), Some("q"));
    assert_eq!(
        found.resource_names,
        Some(vec!["gpu".to_owned(), "disk".to_owned()])
    );
    assert!(found.has_resources && found.has_worktree);
}

#[test]
fn an_ordinary_record_with_the_same_id_marks_the_row_recorded() {
    let w = world();
    put(&w, &launch(A, "reserved"));
    put(&w, &launch(B, "reserved"));
    w.add(&done(A));
    let found = rows(&w, None, 5);
    assert_eq!(
        found.iter().map(|f| f.record).collect::<Vec<_>>(),
        [true, false]
    );
}

#[test]
fn a_refused_receipt_keeps_its_code_and_check_and_no_display_text() {
    let w = world();
    put(&w, &refused(A, "format", "format"));
    let found = rows(&w, None, 5).remove(0);
    assert_eq!(found.state, None);
    assert_eq!(found.refusal, Some(("format".into(), "format".into())));
    assert_eq!(
        (found.title, found.backend, found.generation),
        (None, None, None)
    );
}

#[test]
fn text_that_is_not_utf8_clears_the_value_and_marks_the_text_invalid() {
    for column in [
        "title",
        "backend",
        "repo_path",
        "created_at",
        "not_started_reason",
    ] {
        let w = world();
        put(&w, &launch(A, "reserved"));
        w.raw()
            .execute(
                &format!("UPDATE launch_attempts SET {column}=CAST(x'ff80' AS TEXT) WHERE name=?1"),
                [A],
            )
            .unwrap();
        let found = rows(&w, None, 5).remove(0);
        assert!(!found.text_valid, "{column}");
        let value = match column {
            "title" => found.title,
            "backend" => found.backend,
            "repo_path" => found.repo_path,
            "created_at" => found.created_at,
            _ => found.not_started_reason,
        };
        assert_eq!(value, None, "{column}");
    }
}

#[test]
fn resource_names_that_are_not_all_text_read_as_no_list() {
    let w = world();
    let mut row = launch(A, "reserved");
    row.has_resources = Some(true);
    row.resource_names = Some(vec!["x".into()]);
    put(&w, &row);
    for stored in ["[1]", "[\"a\",null]", "[[\"a\"]]"] {
        w.raw()
            .execute(
                "UPDATE launch_attempts SET resource_names=?1 WHERE name=?2",
                [stored, A],
            )
            .unwrap();
        assert_eq!(rows(&w, None, 1)[0].resource_names, None, "{stored}");
    }
    w.raw()
        .execute(
            "UPDATE launch_attempts SET resource_names='[]' WHERE name=?1",
            [A],
        )
        .unwrap();
    assert_eq!(rows(&w, None, 1)[0].resource_names, Some(Vec::new()));
}

fn row_of(w: &fixture_support::World, id: &str) -> Option<hf_domain::ctl::pending::LaunchFacts> {
    w.session()
        .snapshot(later(5), |s| s.pending_row(&tid(id)))
        .unwrap()
}

#[test]
fn the_row_of_one_id_is_a_launch_a_bare_record_or_nothing() {
    let w = world();
    put(&w, &launch(A, "granted"));
    w.add(&done(B));
    let launched = row_of(&w, A).unwrap();
    assert!(!launched.record);
    assert_eq!(launched.state.as_deref(), Some("granted"));
    let bare = row_of(&w, B).unwrap();
    assert!(bare.record);
    assert_eq!(bare.id, B);
    assert_eq!((bare.state, bare.refusal), (None, None));
    assert!(row_of(&w, C).is_none());
    w.add(&done(A));
    let both = row_of(&w, A).unwrap();
    assert!(both.record);
    assert_eq!(both.state.as_deref(), Some("granted"));
}

#[test]
fn a_prefix_of_an_id_does_not_find_its_row() {
    let w = world();
    put(&w, &launch(A, "granted"));
    assert!(row_of(&w, &A[..12]).is_none());
}

#[test]
fn the_ordinary_lookup_reports_a_refused_receipt_without_a_record() {
    let w = world();
    put(&w, &refused(A, "oversize", "oversize"));
    let lookup = w
        .session()
        .snapshot(later(5), |s| s.lookup(&tid(A)))
        .unwrap();
    assert!(matches!(
        lookup,
        Lookup::LaunchRefused {
            code: "oversize",
            check: "oversize",
            ..
        }
    ));
    w.add(&done(A));
    let lookup = w
        .session()
        .snapshot(later(5), |s| s.lookup(&tid(A)))
        .unwrap();
    assert!(matches!(lookup, Lookup::Found(_)), "a record wins");
}

#[test]
fn a_stored_refusal_outside_the_reference_vocabulary_reads_as_a_malformed_field() {
    let w = world();
    put(&w, &refused(A, "bogus", "unheard_of"));
    let lookup = w
        .session()
        .snapshot(later(5), |s| s.lookup(&tid(A)))
        .unwrap();
    assert!(matches!(
        lookup,
        Lookup::LaunchRefused {
            code: "field_shape",
            check: "field_shape",
            ..
        }
    ));
}

#[test]
fn a_refused_receipt_that_still_qualifies_as_a_document_is_not_a_lookup_refusal() {
    let w = world();
    let mut row = launch(A, "waiting_for_resources");
    row.title = None;
    row.backend = None;
    row.generation = None;
    row.created_at = None;
    row.repo_path = None;
    row.has_resources = Some(true);
    row.refusal_code = Some("field_shape".into());
    row.refusal_check = Some("resource_names".into());
    put(&w, &row);
    let lookup = w
        .session()
        .snapshot(later(5), |s| s.lookup(&tid(A)))
        .unwrap();
    assert!(matches!(lookup, Lookup::Pending(_)), "{lookup:?}");
}

#[test]
fn the_next_name_reads_only_the_name_and_skips_invalid_names() {
    let w = world();
    put(&w, &launch(A, "reserved"));
    put(&w, &invalid("bad name"));
    put(&w, &launch(B, "reserved"));
    assert_eq!(next_name(&w, None).as_deref(), Some(A));
    assert_eq!(next_name(&w, Some(A)).as_deref(), Some(B));
    assert_eq!(next_name(&w, Some(B)), None);
}

#[test]
fn a_record_size_is_its_stored_documents_and_none_without_a_record() {
    let w = world();
    w.add(&done(A));
    let size = |id: &str| {
        w.session()
            .snapshot(later(5), |s| s.record_size(&tid(id)))
            .unwrap()
    };
    // `header:<id>` and `status:<id>` are 43 bytes each.
    assert_eq!(size(A), Some(86));
    assert_eq!(size(B), None);
}

#[test]
fn a_receipt_beyond_the_raw_size_bound_is_refused_undecoded() {
    let w = world();
    let mut big = launch(A, "reserved");
    big.title = Some("t".repeat(65_536));
    w.store.put_launch_attempt(&big, later(5)).unwrap();
    let found = row_of(&w, A).unwrap();
    assert_eq!(found.refusal, Some(("oversize".into(), "oversize".into())));
    assert!(found.title.is_none() && found.state.is_none(), "{found:?}");
}
