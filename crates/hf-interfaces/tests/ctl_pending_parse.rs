#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
//! The arguments, usage errors and help of `task-pending-page` and `task-pending-status`,
//! and their absence on hosts without store reads.
mod ctl_reads_support;
use ctl_reads_support::*;
use hf_domain::ctl::caller::PidObservation;
use hf_interfaces::ctl::{
    cli::{Parsed, PendingPageArgs, PendingStatusArgs, parse_ctl},
    dispatch::{CtlServices, dispatch},
};

fn page(list: &[&str]) -> PendingPageArgs {
    match parse_ctl(&args(list), true).unwrap() {
        Parsed::PendingPage(page) => page,
        other => panic!("{other:?}"),
    }
}

fn status(list: &[&str]) -> PendingStatusArgs {
    match parse_ctl(&args(list), true).unwrap() {
        Parsed::PendingStatus(status) => status,
        other => panic!("{other:?}"),
    }
}

fn message(list: &[&str]) -> (String, String) {
    let error = parse_ctl(&args(list), true).unwrap_err();
    (error.program, error.message)
}

#[test]
fn the_page_defaults_to_fifty_rows_without_a_cursor() {
    let found = page(&["task-pending-page"]);
    assert_eq!((found.json, found.limit, found.cursor), (false, 50, None));
}

#[test]
fn the_page_options_take_their_value_in_both_forms() {
    let found = page(&[
        "task-pending-page",
        "--limit",
        "7",
        "--cursor",
        "c",
        "--json",
    ]);
    assert_eq!(
        (found.json, found.limit, found.cursor.as_deref()),
        (true, 7, Some("c"))
    );
    let found = page(&["task-pending-page", "--limit=7", "--cursor=c=d"]);
    assert_eq!((found.limit, found.cursor.as_deref()), (7, Some("c=d")));
    let found = page(&["task-pending-page", "--json", "--limit=-3"]);
    assert_eq!(found.limit, -3, "the range is the read's to judge");
    let found = page(&["task-pending-page", "--limit", " 8 ", "--cursor", ""]);
    assert_eq!((found.limit, found.cursor.as_deref()), (8, Some("")));
}

#[test]
fn unique_prefixes_of_the_long_options_are_accepted() {
    let found = page(&["task-pending-page", "--li", "9", "--cu=x", "--j"]);
    assert_eq!(
        (found.limit, found.cursor.as_deref(), found.json),
        (9, Some("x"), true)
    );
    let found = status(&["task-pending-status", "--task", "t", "--js"]);
    assert_eq!((found.task_id.as_str(), found.json), ("t", true));
    let found = status(&["task-pending-status", "--t=abc"]);
    assert_eq!(found.task_id, "abc");
}

#[test]
fn the_status_takes_one_exact_task_id_and_leaves_it_untouched() {
    let found = status(&["task-pending-status", "--task-id", "aaaaaaaa"]);
    assert_eq!((found.json, found.task_id.as_str()), (false, "aaaaaaaa"));
    let found = status(&["task-pending-status", "--task-id=--json"]);
    assert_eq!((found.json, found.task_id.as_str()), (false, "--json"));
    let found = status(&["task-pending-status", "--json", "--task-id", ""]);
    assert_eq!(found.task_id, "");
}

#[test]
fn a_missing_required_option_is_a_usage_error() {
    assert_eq!(
        message(&["task-pending-status"]),
        (
            "harflow-ctl task-pending-status".into(),
            "the following arguments are required: --task-id".into()
        )
    );
    assert_eq!(
        message(&["task-pending-status", "--json"]).1,
        "the following arguments are required: --task-id"
    );
}

#[test]
fn bad_values_and_unknown_arguments_are_usage_errors() {
    assert_eq!(
        message(&["task-pending-page", "--limit", "abc"]),
        (
            "harflow-ctl task-pending-page".into(),
            "argument --limit: invalid int value: 'abc'".into()
        )
    );
    assert_eq!(
        message(&["task-pending-page", "--limit"]).1,
        "argument --limit: expected one argument"
    );
    assert_eq!(
        message(&["task-pending-page", "--cursor"]).1,
        "argument --cursor: expected one argument"
    );
    assert_eq!(
        message(&["task-pending-status", "--task-id"]).1,
        "argument --task-id: expected one argument"
    );
    for list in [
        vec!["task-pending-page", "--bogus"],
        vec!["task-pending-page", "extra"],
        vec!["task-pending-page", "--task-id", "x"],
        vec!["task-pending-page", "--json=1"],
        vec!["task-pending-status", "--limit", "1", "--task-id", "x"],
        vec!["task-pending-status", "extra", "--task-id", "x"],
        vec!["task-pending-status", "--cursor", "x", "--task-id", "x"],
        vec!["task-pending-page", "--limit", "1.5"],
    ] {
        assert!(parse_ctl(&args(&list), true).is_err(), "{list:?}");
    }
}

#[test]
fn the_last_repetition_of_an_option_wins() {
    let found = page(&["task-pending-page", "--limit", "3", "--limit", "4"]);
    assert_eq!(found.limit, 4);
    let found = status(&["task-pending-status", "--task-id", "a", "--task-id", "b"]);
    assert_eq!(found.task_id, "b");
}

#[test]
fn help_requests_return_the_usage_of_the_command() {
    assert!(matches!(
        parse_ctl(&args(&["task-pending-page", "--help"]), true),
        Ok(Parsed::Help(Some("task-pending-page")))
    ));
    assert!(matches!(
        parse_ctl(&args(&["task-pending-status", "-h", "--task-id"]), true),
        Ok(Parsed::Help(Some("task-pending-status")))
    ));
}

fn world() -> Fake {
    fake("completed", Some(0), PidObservation::Absent)
}

#[test]
fn the_help_text_is_the_argparse_usage_of_the_reference() {
    let out = dispatch(&args(&["task-pending-page", "--help"]), &world());
    assert_eq!(out.code, 0);
    assert_eq!(
        out.stdout,
        "usage: harflow-ctl task-pending-page [-h] [--json] [--limit LIMIT]\n                                     [--cursor CURSOR]\n"
    );
    let out = dispatch(&args(&["task-pending-status", "-h"]), &world());
    assert_eq!(
        out.stdout,
        "usage: harflow-ctl task-pending-status [-h] [--json] --task-id TASK_ID\n"
    );
}

#[test]
fn usage_errors_exit_with_two_and_a_json_document_only_under_json() {
    let fake = world();
    let out = dispatch(&args(&["task-pending-status"]), &fake);
    assert_eq!(out.code, 2);
    assert_eq!(out.stdout, "");
    assert_eq!(
        out.stderr,
        "harflow-ctl task-pending-status: error: the following arguments are required: --task-id\n"
    );
    let out = dispatch(
        &args(&["task-pending-page", "--limit", "x", "--json"]),
        &fake,
    );
    assert_eq!(out.code, 2);
    assert!(
        out.stdout.contains("invalid int value: 'x'"),
        "{}",
        out.stdout
    );
    assert!(out.stdout.contains("\"error\""), "{}", out.stdout);
}

/// A host whose store reads are unavailable.
struct NoReads(Fake);

impl CtlServices for NoReads {
    fn resolve_state_root(
        &self,
    ) -> Result<hf_domain::ctl::state_root::StateRoot, hf_domain::ctl::state_root::InvalidStateRoot>
    {
        self.0.resolve_state_root()
    }
    fn host_identity(&self) -> hf_domain::ctl::identity::HostIdentity {
        self.0.host_identity()
    }
    fn inspect(
        &self,
        root: &hf_domain::ctl::state_root::StateRoot,
    ) -> hf_domain::ctl::inspect::StateInspection {
        self.0.inspect(root)
    }
}

#[test]
fn on_a_host_without_store_reads_both_commands_are_unknown_choices() {
    let host = NoReads(world());
    for command in [
        vec!["task-pending-page"],
        vec!["task-pending-status", "--task-id", "x"],
        vec!["task-pending-page", "--json"],
    ] {
        let out = dispatch(&args(&command), &host);
        assert_eq!(out.code, 2, "{command:?}");
        assert!(
            out.stderr
                .contains(&format!("invalid choice: '{}'", command[0])),
            "{}",
            out.stderr
        );
        assert!(
            out.stderr
                .ends_with("(choose from 'version-info', 'state-inspect')\n"),
            "not offered: {}",
            out.stderr
        );
    }
    assert!(parse_ctl(&args(&["task-pending-page"]), false).is_err());
    assert!(parse_ctl(&args(&["task-pending-status", "--task-id", "x"]), false).is_err());
}

#[test]
fn with_store_reads_the_invalid_choice_list_offers_both_commands() {
    let (program, text) = message(&["nope"]);
    assert_eq!(program, "harflow-ctl");
    assert!(
        text.contains("'task-pending-page', 'task-pending-status'"),
        "{text}"
    );
}
