#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use hf_interfaces::{
    task_consumer::VersionLine,
    watch_consumer::{ConsumerState, Qualification, RefreshWork, WatchPermit},
};

const ROOT: &str = "/tmp/harflow-fixture-home/.harflow";
const OS: &str = "darwin";
const ARCH: &str = "arm64";
fn line() -> VersionLine {
    VersionLine::new(0, 0).unwrap()
}
fn permit() -> WatchPermit {
    WatchPermit::qualify(
        include_bytes!("../../../tests/fixtures/contracts-v1/version-info.json"),
        Qualification {
            state_root: ROOT,
            os: OS,
            architecture: ARCH,
            version_line: line(),
        },
    )
    .unwrap()
}

#[test]
fn consumer_rejects_malformed_success_shapes_and_accepts_unknown_additive_fields() {
    let baseline = serde_json::from_slice::<serde_json::Value>(include_bytes!(
        "../../../tests/fixtures/contracts-v1/ctl-task-list-watch-baseline.json"
    ))
    .unwrap();
    let mut additive = baseline.clone();
    additive["result"]["future_field"] = serde_json::json!({"x": true});
    assert!(
        permit()
            .consume(&serde_json::to_vec(&additive).unwrap())
            .is_ok()
    );

    for (field, value) in [
        ("state", serde_json::json!(true)),
        ("cursor", serde_json::json!(null)),
        ("reason", serde_json::json!("unexpected")),
    ] {
        let mut invalid = baseline.clone();
        invalid["result"][field] = value;
        assert!(
            permit()
                .consume(&serde_json::to_vec(&invalid).unwrap())
                .is_err(),
            "{field}"
        );
    }
    let mut invalid = baseline;
    invalid["result"]["cursor"] = serde_json::json!("x".repeat(2049));
    assert!(
        permit()
            .consume(&serde_json::to_vec(&invalid).unwrap())
            .is_err()
    );
}

#[test]
fn consumer_retains_cursor_and_coalesces_refresh_work_while_busy() {
    let mut state = ConsumerState::default();
    assert_eq!(state.begin_wait().unwrap(), None);
    let baseline = permit()
        .consume(include_bytes!(
            "../../../tests/fixtures/contracts-v1/ctl-task-list-watch-baseline.json"
        ))
        .unwrap();
    state.accept(&baseline).unwrap();
    assert_eq!(state.take_refresh(), Some(RefreshWork::Snapshot));
    let retained = state.cursor().unwrap().to_owned();
    state.begin_wait().unwrap();
    assert!(state.begin_wait().is_err());
    let changed = permit()
        .consume(include_bytes!(
            "../../../tests/fixtures/contracts-v1/ctl-task-list-watch-changed.json"
        ))
        .unwrap();
    state.accept(&changed).unwrap();
    assert_eq!(state.cursor(), changed.cursor());
    assert_ne!(state.cursor(), Some(retained.as_str()));
    assert_eq!(state.take_refresh(), None);
    state.begin_wait().unwrap();
    let timeout = permit()
        .consume(include_bytes!(
            "../../../tests/fixtures/contracts-v1/ctl-task-list-watch-timeout.json"
        ))
        .unwrap();
    state.accept(&timeout).unwrap();
    state.finish_refresh();
    assert_eq!(state.take_refresh(), Some(RefreshWork::Reconcile));
}

#[test]
fn initial_reset_recovers_a_cursor_before_snapshot_refresh() {
    let mut raw: serde_json::Value = serde_json::from_slice(include_bytes!(
        "../../../tests/fixtures/contracts-v1/ctl-task-list-watch-reset.json"
    ))
    .unwrap();
    raw["result"]["reason"] = serde_json::json!("open_reclaimed");
    let reset = permit()
        .consume(&serde_json::to_vec(&raw).unwrap())
        .unwrap();
    let mut state = ConsumerState::default();
    assert_eq!(state.begin_wait().unwrap(), None);
    state.accept(&reset).unwrap();
    assert_eq!(state.cursor(), reset.cursor());
    assert_eq!(state.take_refresh(), Some(RefreshWork::Snapshot));
}

#[test]
fn additive_string_error_codes_remain_refusals() {
    for code in ["vendor.error-v2", "", &"x".repeat(129)] {
        let mut raw: serde_json::Value = serde_json::from_slice(include_bytes!(
            "../../../tests/fixtures/contracts-v1/ctl-task-list-watch-cursor-invalid.json"
        ))
        .unwrap();
        raw["error"]["code"] = serde_json::json!(code);
        let document = permit()
            .consume(&serde_json::to_vec(&raw).unwrap())
            .unwrap();
        assert_eq!(document.state(), None);
        assert_eq!(document.raw()["error"]["code"], code);
    }
}
