#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
//! The pure policy of pending launch discovery: text cuts, resource names, the disposition
//! of a stored row and the vocabulary of refusals.
use hf_domain::ctl::pending::{
    LaunchFacts, LaunchOutcome, LaunchView, cut, document_reason, names_valid, outcome, refusal_of,
};

const A: &str = "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa";

fn facts(state: &str) -> LaunchFacts {
    LaunchFacts {
        id: A.into(),
        state: Some(state.into()),
        generation: Some("g".into()),
        backend: Some("claude".into()),
        text_valid: true,
        ..LaunchFacts::default()
    }
}

fn pending(facts: &LaunchFacts) -> LaunchView {
    match outcome(facts) {
        LaunchOutcome::Pending(view) => *view,
        other => panic!("{other:?}"),
    }
}

fn names(list: &[&str]) -> Vec<String> {
    list.iter().map(|name| (*name).to_owned()).collect()
}

#[test]
fn a_cut_never_splits_a_character() {
    assert_eq!(cut("abc", 3), "abc");
    assert_eq!(cut("abcd", 3), "abc");
    assert_eq!(cut("", 0), "");
    // "é" is two bytes: a cut inside it backs up to the boundary.
    assert_eq!(cut("aé", 2), "a");
    assert_eq!(cut("aé", 3), "aé");
    assert_eq!(cut("😀😀", 5), "😀");
    assert_eq!(cut("😀", 0), "");
}

#[test]
fn resource_names_follow_the_reference_rules() {
    assert!(names_valid(&names(&["a"])));
    assert!(names_valid(&names(&["a.b_c:d-e", "0x"])));
    assert!(!names_valid(&[]));
    assert!(!names_valid(&names(&[""])));
    assert!(!names_valid(&names(&["A"])), "uppercase");
    assert!(
        !names_valid(&names(&["-a"])),
        "must start with a letter or digit"
    );
    assert!(!names_valid(&names(&[".a"])));
    assert!(!names_valid(&names(&["a b"])));
    assert!(!names_valid(&names(&["é"])));
    assert!(!names_valid(&names(&["a", "a"])), "distinct");
    assert!(names_valid(&["a".repeat(64)]));
    assert!(!names_valid(&["a".repeat(65)]));
    let sixteen: Vec<String> = (0..16).map(|n| format!("n{n}")).collect();
    assert!(names_valid(&sixteen));
    let seventeen: Vec<String> = (0..17).map(|n| format!("n{n}")).collect();
    assert!(!names_valid(&seventeen));
}

#[test]
fn a_record_wins_over_every_receipt_state() {
    let mut row = facts("spawned");
    row.record = true;
    assert_eq!(outcome(&row), LaunchOutcome::Recorded);
    row.state = None;
    row.refusal = Some(("format".into(), "format".into()));
    assert_eq!(outcome(&row), LaunchOutcome::Recorded);
    assert_eq!(
        outcome(&LaunchFacts {
            record: true,
            ..LaunchFacts::default()
        }),
        LaunchOutcome::Recorded
    );
}

#[test]
fn an_unqualified_document_is_refused_with_its_stored_code() {
    let mut row = LaunchFacts {
        id: A.into(),
        refusal: Some(("oversize".into(), "oversize".into())),
        ..LaunchFacts::default()
    };
    assert_eq!(
        outcome(&row),
        LaunchOutcome::Refused {
            code: "oversize",
            check: "oversize"
        }
    );
    row.refusal = None;
    assert_eq!(
        outcome(&row),
        LaunchOutcome::Refused {
            code: "field_shape",
            check: "field_shape"
        }
    );
    row.refusal = Some(("made_up".into(), "format".into()));
    assert_eq!(
        outcome(&row),
        LaunchOutcome::Refused {
            code: "field_shape",
            check: "field_shape"
        }
    );
}

#[test]
fn spawned_and_settled_receipts_without_a_record_are_inconsistent() {
    for state in ["spawned", "settled"] {
        assert_eq!(
            outcome(&facts(state)),
            LaunchOutcome::Inconsistent,
            "{state}"
        );
    }
    // Even with invalid display text: the state is judged first.
    let mut row = facts("spawned");
    row.text_valid = false;
    assert_eq!(outcome(&row), LaunchOutcome::Inconsistent);
}

#[test]
fn display_text_that_is_not_utf8_is_refused_before_a_stored_refusal() {
    let mut row = facts("reserved");
    row.text_valid = false;
    assert_eq!(
        outcome(&row),
        LaunchOutcome::Refused {
            code: "field_shape",
            check: "display_text_encoding"
        }
    );
    row.refusal = Some(("field_shape".into(), "resource_names".into()));
    assert_eq!(
        outcome(&row),
        LaunchOutcome::Refused {
            code: "field_shape",
            check: "display_text_encoding"
        }
    );
    row.text_valid = true;
    assert_eq!(
        outcome(&row),
        LaunchOutcome::Refused {
            code: "field_shape",
            check: "resource_names"
        }
    );
}

#[test]
fn resource_names_are_judged_only_when_the_receipt_has_resources() {
    let mut row = facts("waiting_for_resources");
    row.resource_names = Some(names(&["Bad Name"]));
    assert!(
        matches!(outcome(&row), LaunchOutcome::Pending(_)),
        "no resources object"
    );
    row.has_resources = true;
    let refused = LaunchOutcome::Refused {
        code: "field_shape",
        check: "resource_names",
    };
    assert_eq!(outcome(&row), refused);
    row.resource_names = None;
    assert_eq!(outcome(&row), refused, "no list of strings");
    row.resource_names = Some(Vec::new());
    assert_eq!(outcome(&row), refused, "an empty list");
    row.resource_names = Some(names(&["gpu"]));
    assert_eq!(pending(&row).resource_names, ["gpu"]);
}

#[test]
fn cancel_support_needs_resources_or_a_worktree_and_a_started_state() {
    for (state, resources, worktree, wanted) in [
        ("reserved", false, false, false),
        ("granted", true, false, true),
        ("granted", false, true, true),
        ("not_started", true, true, false),
        ("not_started", false, false, false),
    ] {
        let mut row = facts(state);
        row.has_resources = resources;
        row.has_worktree = worktree;
        row.resource_names = resources.then(|| names(&["gpu"]));
        assert_eq!(
            pending(&row).cancel_support,
            wanted,
            "{state} {resources} {worktree}"
        );
    }
}

#[test]
fn the_view_keeps_an_empty_reason_and_only_valid_lineage_in_field_order() {
    let mut row = facts("not_started");
    row.not_started_reason = Some(String::new());
    row.spawned_by = Some("s-1".into());
    row.root_task_id = Some("not valid!".into());
    row.parent_task_id = Some("p".into());
    row.chain_root_task_id = None;
    let view = pending(&row);
    assert_eq!(view.not_started_reason.as_deref(), Some(""));
    assert_eq!(
        view.lineage,
        [
            ("spawned_by", "s-1".to_owned()),
            ("parent_task_id", "p".to_owned())
        ]
    );
    assert_eq!(
        (view.generation.as_str(), view.state.as_str()),
        ("g", "not_started")
    );
    assert!(!view.metadata_truncated);
    assert_eq!(view.title, None);
}

#[test]
fn text_is_cut_at_its_limits_and_the_cut_is_reported() {
    let at = |len: usize| "é".repeat(len / 2);
    let mut row = facts("reserved");
    row.title = Some(at(1024));
    row.backend = Some(at(1024));
    row.created_at = Some(at(1024));
    row.not_started_reason = Some(at(1024));
    row.repo_path = Some("p".repeat(4096));
    let view = pending(&row);
    assert!(!view.metadata_truncated, "exactly at the limits");
    row.title = Some("é".repeat(513));
    let view = pending(&row);
    assert!(view.metadata_truncated);
    assert_eq!(view.title.as_deref().unwrap().len(), 1024);
    row.title = None;
    row.repo_path = Some("p".repeat(4097));
    let view = pending(&row);
    assert!(view.metadata_truncated);
    assert_eq!(view.repo_path.as_deref().unwrap().len(), 4096);
    row.repo_path = None;
    for field in 0..3 {
        let mut long = facts("reserved");
        let value = Some("x".repeat(1025));
        match field {
            0 => long.backend = value,
            1 => long.created_at = value,
            _ => long.not_started_reason = value,
        }
        assert!(pending(&long).metadata_truncated, "{field}");
    }
}

#[test]
fn resource_names_are_capped_in_number_and_length() {
    let mut row = facts("granted");
    row.has_resources = true;
    row.resource_names = Some((0..16).map(|n| format!("n{n}")).collect());
    assert!(!pending(&row).metadata_truncated);
    // The view caps the list at 100 names and each name at 256 bytes. A receipt with
    // resources never reaches those caps (validation allows 16 names of 64 bytes), so the
    // caps are exercised on facts that carry names without a resources object.
    let mut worktree = facts("granted");
    worktree.has_worktree = true;
    worktree.resource_names = Some((0..101).map(|n| format!("n{n}")).collect());
    let view = pending(&worktree);
    assert_eq!(view.resource_names.len(), 100);
    assert!(view.metadata_truncated);
    worktree.resource_names = Some(vec!["a".repeat(256)]);
    assert!(!pending(&worktree).metadata_truncated);
    worktree.resource_names = Some(vec!["é".repeat(129)]);
    let view = pending(&worktree);
    assert!(view.metadata_truncated);
    assert_eq!(view.resource_names[0].len(), 256);
}

#[test]
fn refusals_outside_the_vocabulary_read_as_a_malformed_field() {
    assert_eq!(refusal_of("format", "format"), ("format", "format"));
    assert_eq!(
        refusal_of("field_shape", "resource_names"),
        ("field_shape", "resource_names")
    );
    assert_eq!(
        refusal_of("field_shape", "display_text_encoding"),
        ("field_shape", "display_text_encoding")
    );
    assert_eq!(
        refusal_of("task_unavailable", "source_changed"),
        ("task_unavailable", "source_changed")
    );
    assert_eq!(refusal_of("nope", "format"), ("field_shape", "field_shape"));
    assert_eq!(refusal_of("format", "nope"), ("field_shape", "field_shape"));
    assert_eq!(refusal_of("", ""), ("field_shape", "field_shape"));
}

#[test]
fn every_check_has_the_reference_reason_text() {
    for (check, reason) in [
        ("format", "Incompatible task launch format"),
        ("version", "Incompatible task launch version"),
        (
            "root_mismatch",
            "Task launch root differs from selected root",
        ),
        (
            "task_id_mismatch",
            "Task launch task ID differs from source name",
        ),
        ("generation", "Invalid launch generation"),
        ("owner_shape", "Malformed launch owner identity"),
        ("oversize", "Task launch exceeds 65536 bytes"),
        (
            "identity_mismatch",
            "Task launch directory binding differs from selected tasks directory",
        ),
        ("not_regular", "Task launch source is not a regular file"),
        ("source_changed", "Task launch changed while reading"),
        (
            "withdrawal_claim",
            "Withdrawal no-spawn claim cannot be qualified",
        ),
        (
            "withdrawal_proof",
            "Withdrawal terminal proof cannot be qualified",
        ),
        ("field_shape", "Malformed task launch field"),
        ("resource_names", "Malformed task launch field"),
        ("display_text_encoding", "Malformed task launch field"),
        ("anything else", "Malformed task launch field"),
    ] {
        assert_eq!(document_reason(check), reason, "{check}");
    }
}
