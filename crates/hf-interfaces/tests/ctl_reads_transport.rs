#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
//! Rendering of the store-backed ctl reads through `dispatch` over scripted ports.
mod ctl_reads_support;
use ctl_reads_support::*;
use hf_domain::ctl::caller::PidObservation;
use hf_interfaces::ctl::dispatch::dispatch;
use serde_json::Value;

#[test]
fn alive_and_running_does_not_need_reconciliation_and_the_blob_never_wins() {
    let alive = fake("running", None, PidObservation::Present(identity(40)));
    let doc = task(&alive, &[]);
    assert_eq!(doc["needs_reconciliation"], false);
    assert_eq!(doc["process_identity_state"], "alive");
    assert_eq!(doc["status"], "running");
    let dead = fake("running", None, PidObservation::Absent);
    let doc = task(&dead, &[]);
    assert_eq!(
        (
            doc["status"].as_str(),
            doc["process_identity_state"].as_str()
        ),
        (Some("unknown"), Some("dead"))
    );
    assert_eq!(doc["needs_reconciliation"], true);
    let settled = fake("completed", Some(0), PidObservation::Present(identity(40)));
    let doc = task(&settled, &[]);
    assert_eq!(
        (
            doc["needs_reconciliation"].clone(),
            doc["process_identity_state"].as_str()
        ),
        (false.into(), Some("dead"))
    );
    assert_eq!(doc["observed_exit"], true);
}

#[test]
fn observer_parent_fields() {
    let alive = fake("completed", Some(0), PidObservation::Absent);
    let doc = task(&alive, &["--observer-parent"]);
    assert_eq!(doc["observer_kind"], "parent_process");
    assert_eq!(doc["observer_state"], "alive");
    assert_eq!(doc["observing_process"]["pid"], 77);
    assert_eq!(
        doc["observing_process"]["start_time"],
        "2023-11-14T22:13:20.123456+00:00"
    );
    assert!(doc.get("observation_note").is_none());
    let mut gone = fake("completed", Some(0), PidObservation::Absent);
    gone.parent = None;
    let doc = task(&gone, &["--observer-parent"]);
    assert_eq!(
        (
            doc["observer_state"].as_str(),
            doc["observing_process"].is_null()
        ),
        (Some("undecidable"), true)
    );
    assert!(
        doc["observation_note"]
            .as_str()
            .is_some_and(|n| !n.is_empty())
    );
    assert!(task(&gone, &[]).get("observer_kind").is_none());
}

#[test]
fn a_page_withheld_for_damage_has_exactly_the_documented_keys() {
    let mut damaged = fake("completed", Some(0), PidObservation::Absent);
    damaged.damaged = true;
    let out = dispatch(&args(&["task-list-page", "--json"]), &damaged);
    let doc: Value = serde_json::from_str(&out.stdout).unwrap();
    let mut keys: Vec<&str> = doc["result"]
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    keys.sort_unstable();
    assert_eq!(
        keys,
        [
            "authority_incomplete",
            "bootstrap_pending",
            "catalog_state",
            "counts_complete",
            "has_more",
            "history_incomplete",
            "items",
            "next_cursor",
            "note",
            "related_headers"
        ]
    );
    assert_eq!(doc["result"]["catalog_state"]["status"], "blocked");
}

#[test]
fn a_blob_that_disagrees_with_the_facts_fails_closed() {
    for extra in [
        r#","backend":"other""#,
        r#","exit_code":5"#,
        r#","title":"x""#,
    ] {
        let mut bad = fake("completed", Some(0), PidObservation::Absent);
        bad.header_extra = extra;
        for command in [vec!["list", "--json"], vec!["status", ID, "--json"]] {
            let out = dispatch(&args(&command), &bad);
            assert_eq!(out.code, 1, "{extra}");
            let doc: Value = serde_json::from_str(&out.stdout).unwrap();
            assert_eq!(doc["error"]["code"], "state_incompatible");
            assert_eq!(doc["error"]["data"]["reason"], "infrastructure_unavailable");
        }
    }
    let mut fine = fake("completed", Some(0), PidObservation::Absent);
    fine.header_extra = r#","backend":"claude""#;
    assert_eq!(dispatch(&args(&["list", "--json"]), &fine).code, 0);
}

/// observation fields follow a rule and a disagreeing blob is refused.
#[test]
fn a_blob_with_other_observation_fields_is_refused() {
    for extra in [r#","recovered":false"#, r#","observation_mode":"live""#] {
        let mut bad = fake("completed", Some(0), PidObservation::Absent);
        bad.header_extra = extra;
        let out = dispatch(&args(&["status", ID, "--json"]), &bad);
        assert_eq!(out.code, 1, "{extra}");
    }
    let doc = task(&fake("completed", Some(0), PidObservation::Absent), &[]);
    assert_eq!(
        (doc["recovered"].clone(), doc["observation_mode"].as_str()),
        (true.into(), Some("unknown"))
    );
}

/// no stored `active` key reaches a document.
#[test]
fn a_stored_active_key_is_not_rendered() {
    let mut stale = fake("completed", Some(0), PidObservation::Absent);
    stale.header_extra = r#","active":true"#;
    let doc = task(&stale, &[]);
    assert!(doc.get("active").is_none(), "{doc}");
}

#[test]
fn errors_use_exit_one_with_the_envelope_only_under_json() {
    let mut damaged = fake("completed", Some(0), PidObservation::Absent);
    damaged.damaged = true;
    let quiet = dispatch(&args(&["list"]), &damaged);
    assert_eq!((quiet.code, quiet.stdout.as_str()), (1, ""));
    assert!(quiet.stderr.starts_with("harflow-ctl: error: "));
    let loud = dispatch(&args(&["list", "--js"]), &damaged);
    assert_eq!(loud.code, 1);
    assert_eq!(loud.stderr, quiet.stderr);
    let doc: Value = serde_json::from_str(&loud.stdout).unwrap();
    assert_eq!(doc["error"]["code"], "workflow_read_refused");
}

#[test]
fn pretty_output_without_json_has_no_envelope() {
    let alive = fake("completed", Some(0), PidObservation::Absent);
    let out = dispatch(&args(&["status", ID]), &alive);
    assert_eq!(out.code, 0);
    assert!(out.stdout.lines().count() > 3);
    let doc: Value = serde_json::from_str(&out.stdout).unwrap();
    assert!(doc.get("product").is_none() && doc["task_id"] == ID);
}

#[test]
fn since_without_a_clock_is_an_error_never_a_wider_window() {
    let mut fake = fake("completed", Some(0), PidObservation::Absent);
    fake.clock = false;
    let out = dispatch(&args(&["list", "--since", "1d", "--json"]), &fake);
    assert_eq!(out.code, 1, "{}", out.stdout);
    let doc: Value = serde_json::from_str(&out.stdout).unwrap();
    assert!(doc["error"]["message"].as_str().unwrap().contains("clock"));
    assert!(doc.get("tasks").is_none());
    let plain = dispatch(&args(&["list", "--json"]), &fake);
    assert_eq!(plain.code, 0, "no window needs no clock");
}
