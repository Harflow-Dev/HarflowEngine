#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
//! Argument parsing and the wire cursor of the store-backed ctl reads.
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use hf_domain::ctl::task_rows::WireCursor;
use hf_interfaces::ctl::{
    cli::{Parsed, parse_ctl},
    cursor,
};

const ID: &str = "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa";

fn args(list: &[&str]) -> Vec<String> {
    list.iter().map(|s| s.to_string()).collect()
}

fn json_flag(list: &[&str]) -> bool {
    match parse_ctl(&args(list), true).unwrap() {
        Parsed::List { json, .. }
        | Parsed::Status { json, .. }
        | Parsed::StatusBatch { json, .. } => json,
        Parsed::Page(page) => page.json,
        other => panic!("{other:?}"),
    }
}

#[test]
fn only_a_flag_resolving_to_json_sets_json() {
    for list in [["list", "--json"], ["list", "--js"], ["list", "--j"]] {
        assert!(json_flag(&list), "{list:?}");
    }
    assert!(!json_flag(&["list", "--since", "7d"]));
    assert!(!json_flag(&["task-list-page", "--active-only"]));
    assert!(!json_flag(&["task-list-page", "--cursor=--json"]));
    assert!(!json_flag(&["status", "--", "--json"]));
    assert!(json_flag(&["status", ID, "--observer-parent", "--json"]));
}

#[test]
fn page_options_and_usage_errors() {
    let Parsed::Page(page) = parse_ctl(
        &args(&[
            "task-list-page",
            "--limit=7",
            "--cursor",
            "c",
            "--active-only",
            "--session-id",
            "s",
            "--task-ids",
            "a,b",
        ]),
        true,
    )
    .unwrap() else {
        panic!()
    };
    assert_eq!(
        (page.limit, page.cursor.as_deref(), page.active_only),
        (7, Some("c"), true)
    );
    assert_eq!(
        (page.session_id.as_deref(), page.task_ids.as_deref()),
        (Some("s"), Some("a,b"))
    );
    let Parsed::Page(page) = parse_ctl(&args(&["task-list-page"]), true).unwrap() else {
        panic!()
    };
    assert_eq!(page.limit, 100);
    for bad in [
        vec!["task-list-page", "--limit", "abc"],
        vec!["task-list-page", "--limit"],
        vec!["task-list-page", "--bogus"],
        vec!["status"],
        vec!["status", ID, ID],
        vec!["status-batch"],
        vec!["list", "--since", "bogus"],
        vec!["list", "--since", "-3d"],
        vec![
            "task-list-page",
            "--predecessor-task-id",
            ID,
            "--task-ids",
            ID,
        ],
        vec!["list", "--json=1"],
    ] {
        assert!(parse_ctl(&args(&bad), true).is_err(), "{bad:?}");
    }
    assert!(
        parse_ctl(&args(&["list"]), false).is_err(),
        "no reads off platform"
    );
}

fn cursor_value() -> WireCursor {
    WireCursor {
        scope: "ab".repeat(32),
        stamp: -5,
        id: ID.to_string(),
    }
}

#[test]
fn cursor_round_trips_and_rejects_malformed_values() {
    let wire = cursor_value();
    assert_eq!(cursor::decode(&cursor::encode(&wire)), Some(wire.clone()));
    let body = |text: &str| URL_SAFE_NO_PAD.encode(text);
    let good = format!(
        r#"{{"v":1,"scope":"{}","stamp":3,"id":"{ID}"}}"#,
        wire.scope
    );
    assert!(cursor::decode(&body(&good)).is_some());
    for bad in [
        String::new(),
        "bogus".into(),
        body("[]"),
        body("{}"),
        body(&good.replace("\"v\":1", "\"v\":2")),
        body(&good.replace("\"stamp\":3", "\"stamp\":3.5")),
        body(&good.replace(ID, "bad/id")),
        body(&good.replace(&wire.scope, "zz")),
        body(&good.replace("\"v\"", "\"x\":1,\"v\"")),
    ] {
        assert_eq!(cursor::decode(&bad), None, "{bad}");
    }
    let padded = |extra: usize| body(&format!("{good}{}", " ".repeat(extra)));
    assert!(cursor::decode(&padded(1000)).is_some());
    assert!(cursor::decode(&padded(2000)).is_none());
    let exact = (0..3000).find(|n| padded(*n).len() == 2048).unwrap();
    assert!(cursor::decode(&padded(exact)).is_some());
    assert!(cursor::decode(&padded(exact + 3)).is_none());
}
