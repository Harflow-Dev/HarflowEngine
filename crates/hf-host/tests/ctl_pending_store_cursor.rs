#![cfg(all(target_os = "macos", target_arch = "aarch64"))]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

//! The life of a `task-pending-page` cursor on real roots: content changes keep it, a changed
//! set of names expires it, and another limit or store refuses it.
mod ctl_reads_store_support;
use ctl_reads_store_support::{pending::*, *};
use std::path::Path;

#[test]
fn a_cursor_survives_a_change_of_content_but_not_a_new_or_removed_name() {
    let _one = exclusive();
    let scratch = scratch("cursor-life");
    let root = root_of(&scratch, "r", &[launch(1), launch(2), launch(3)], &[]);
    let cursor = first_cursor(&root);
    let resume = |root: &Path| page(root, &["--limit", "1", "--cursor", &cursor]);
    let (code, body) = resume(&root);
    assert_eq!(code, Some(0), "{body}");
    assert_eq!(listed(&body), [id(2).as_str()]);

    let writer = reopen(&root);
    let mut changed = launch(2);
    changed.title = Some("Another title".into());
    changed.state = Some("granted".into());
    writer.put_launch_attempt(&changed, later()).unwrap();
    drop(writer);
    let (code, body) = resume(&root);
    assert_eq!(code, Some(0), "content only: {body}");
    assert!(body.contains("\"state\":\"granted\""), "{body}");

    let writer = reopen(&root);
    writer.put_launch_attempt(&launch(4), later()).unwrap();
    drop(writer);
    expect_check_with(
        &resume(&root),
        "pending_cursor_expired",
        "namespace_changed",
    );

    let writer = reopen(&root);
    writer
        .replace_launch_fixture(&[launch(1), launch(2), launch(3)], later())
        .unwrap();
    drop(writer);
    let (code, body) = resume(&root);
    assert_eq!(code, Some(0), "back to the same names: {body}");
}

#[test]
fn removing_the_launch_a_cursor_stands_on_expires_it() {
    let _one = exclusive();
    let scratch = scratch("cursor-removal");
    let root = root_of(&scratch, "r", &[launch(1), launch(2)], &[]);
    let cursor = first_cursor(&root);
    let writer = reopen(&root);
    writer
        .replace_launch_fixture(&[launch(2)], later())
        .unwrap();
    drop(writer);
    let result = page(&root, &["--limit", "1", "--cursor", &cursor]);
    expect_check_with(&result, "pending_cursor_expired", "namespace_changed");
}

#[test]
fn a_cursor_of_another_limit_or_store_is_refused() {
    let _one = exclusive();
    let scratch = scratch("cursor-scope");
    let rows = [launch(1), launch(2)];
    let root = root_of(&scratch, "r", &rows, &[]);
    let cursor = first_cursor(&root);
    let other_limit = page(&root, &["--limit", "2", "--cursor", &cursor]);
    expect_check_with(&other_limit, "pending_cursor_invalid", "cursor_scope");
    let default_limit = page(&root, &["--cursor", &cursor]);
    expect_check_with(&default_limit, "pending_cursor_invalid", "cursor_scope");
    // The same names in another store: another root, so another scope.
    let other = root_of(&scratch, "other", &rows, &[]);
    let elsewhere = page(&other, &["--limit", "1", "--cursor", &cursor]);
    expect_check_with(&elsewhere, "pending_cursor_invalid", "cursor_scope");
    let garbage = page(&root, &["--limit", "1", "--cursor", "garbage"]);
    expect_check_with(&garbage, "pending_cursor_invalid", "cursor_shape");
}

#[test]
fn limits_outside_one_to_one_hundred_are_refused_after_the_gate() {
    let _one = exclusive();
    let scratch = scratch("limits");
    let root = root_of(&scratch, "r", &[launch(1)], &[]);
    for limit in ["0", "101", "-5"] {
        expect_check(
            &root,
            &["--limit", limit],
            "invalid_pending_limit",
            "limit_1_to_100",
        );
    }
    let (code, body) = page(&root, &["--limit", "100"]);
    assert_eq!(code, Some(0), "{body}");
}
