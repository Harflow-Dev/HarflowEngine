#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use hf_application::task_watch::{WatchCodecPort, WatchError};
use hf_domain::task_watch::{WatchCursor, WatchReason, WatchResult, WatchState};
use hf_interfaces::{
    task_consumer::VersionLine,
    watch_codec::WatchJsonCodec,
    watch_consumer::{Qualification, WatchPermit, encode_result},
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
fn canonical_watch_documents_and_missing_capability_are_qualified_per_operation() {
    for (state, bytes) in [
        (
            "baseline",
            include_bytes!(
                "../../../tests/fixtures/contracts-v1/ctl-task-list-watch-baseline.json"
            )
            .as_slice(),
        ),
        (
            "changed",
            include_bytes!("../../../tests/fixtures/contracts-v1/ctl-task-list-watch-changed.json")
                .as_slice(),
        ),
        (
            "timeout",
            include_bytes!("../../../tests/fixtures/contracts-v1/ctl-task-list-watch-timeout.json")
                .as_slice(),
        ),
        (
            "reset",
            include_bytes!("../../../tests/fixtures/contracts-v1/ctl-task-list-watch-reset.json")
                .as_slice(),
        ),
        (
            "unavailable",
            include_bytes!(
                "../../../tests/fixtures/contracts-v1/ctl-task-list-watch-unavailable.json"
            )
            .as_slice(),
        ),
    ] {
        let handshake = permit();
        let document = handshake.consume(bytes).unwrap();
        assert_eq!(document.state(), Some(state), "{state}");
        assert_eq!(document.original_bytes(), bytes, "{state}");
    }
    for (name, bytes) in [
        ("cursor-invalid", include_bytes!("../../../tests/fixtures/contracts-v1/ctl-task-list-watch-cursor-invalid.json").as_slice()),
        ("direct-caller-refused", include_bytes!("../../../tests/fixtures/contracts-v1/ctl-task-list-watch-direct-caller-refused.json").as_slice()),
    ] {
        let document = permit().consume(bytes).unwrap();
        assert!(document.raw().get("error").is_some(), "{name}");
    }
    assert!(
        WatchPermit::qualify(
            include_bytes!(
                "../../../tests/fixtures/contracts-v1/invalid-s05-missing-task-list-watch.json"
            ),
            Qualification {
                state_root: ROOT,
                os: OS,
                architecture: ARCH,
                version_line: line()
            },
        )
        .is_err()
    );
}

#[test]
fn handshake_mutations_require_exact_identity_and_watch_capability() {
    let original: serde_json::Value = serde_json::from_slice(include_bytes!(
        "../../../tests/fixtures/contracts-v1/version-info.json"
    ))
    .unwrap();
    for (path, value) in [
        ("state_root", serde_json::json!("/foreign")),
        ("os", serde_json::json!("linux")),
        ("architecture", serde_json::json!("x86_64")),
        (
            "capabilities",
            serde_json::json!(["ctl.v1", "version-info.v1"]),
        ),
    ] {
        let mut changed = original.clone();
        changed[path] = value;
        assert!(
            WatchPermit::qualify(
                &serde_json::to_vec(&changed).unwrap(),
                Qualification {
                    state_root: ROOT,
                    os: OS,
                    architecture: ARCH,
                    version_line: line(),
                }
            )
            .is_err(),
            "{path}"
        );
    }
    let mut additive = original;
    additive["capabilities"]
        .as_array_mut()
        .unwrap()
        .push(serde_json::json!("future.extra.v1"));
    assert!(
        WatchPermit::qualify(
            &serde_json::to_vec(&additive).unwrap(),
            Qualification {
                state_root: ROOT,
                os: OS,
                architecture: ARCH,
                version_line: line(),
            }
        )
        .is_ok()
    );
}

#[test]
fn cursor_codec_rejects_wrong_scope_root_and_non_integer_or_overflow_scalars() {
    let codec = WatchJsonCodec;
    let valid = WatchCursor {
        root: "a".repeat(32),
        epoch: "b".repeat(32),
        revision: i64::MAX,
    };
    let encoded = codec.encode(&valid).unwrap();
    assert_eq!(codec.decode(&encoded, &valid.root).unwrap(), valid);
    assert_eq!(
        codec.decode(&encoded, &"c".repeat(32)),
        Err(WatchError::InvalidCursor)
    );
    for cursor in ["not-base64", &"x".repeat(2049), "é"] {
        assert_eq!(
            codec.decode(cursor, &valid.root),
            Err(WatchError::InvalidCursor)
        );
    }
    for cursor in [
        "eyJ2IjoxLCJzY29wZSI6InRhc2stbGlzdC13YXRjaC52MSIsInJvb3QiOiJhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYSIsImVwb2NoIjoiYmJiYmJiYmJiYmJiYmJiYmJiYmJiYmJiYmJiYmJiYmIiLCJyZXZpc2lvbiI6dHJ1ZX0=",
        "eyJ2IjoxLCJzY29wZSI6InRhc2stbGlzdC13YXRjaC52MSIsInJvb3QiOiJhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYSIsImVwb2NoIjoiYmJiYmJiYmJiYmJiYmJiYmJiYmJiYmJiYmJiYmJiYmIiLCJyZXZpc2lvbiI6OTIyMzM3MjAzNjg1NDc3NTgwOH0=",
        "eyJ2IjoxLCJzY29wZSI6InRhc2stbGlzdC13YXRjaC52MSIsInJvb3QiOiJhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYSIsImVwb2NoIjoiYmJiYmJiYmJiYmJiYmJiYmJiYmJiYmJiYmJiYmJiYmIiLCJyZXZpc2lvbiI6LTF9",
    ] {
        assert_eq!(
            codec.decode(cursor, &valid.root),
            Err(WatchError::InvalidCursor)
        );
    }
}

#[test]
fn producer_result_mapping_enforces_state_cursor_and_reason_shape() {
    let cursor = WatchCursor {
        root: "a".repeat(32),
        epoch: "b".repeat(32),
        revision: 1,
    };
    let baseline = WatchResult {
        state: WatchState::Baseline,
        cursor: Some(WatchJsonCodec.encode(&cursor).unwrap()),
        reason: None,
    };
    let bytes = encode_result(&baseline, "0.0.10").unwrap();
    assert_eq!(permit().consume(&bytes).unwrap().state(), Some("baseline"));
    let missing = WatchResult {
        state: WatchState::Unavailable,
        cursor: None,
        reason: Some(WatchReason::StateMissing),
    };
    assert_eq!(
        permit()
            .consume(&encode_result(&missing, "0.0.10").unwrap())
            .unwrap()
            .state(),
        Some("unavailable")
    );
    assert!(encode_result(&baseline, "0.0.1").is_ok());
    let invalid = WatchResult {
        state: WatchState::Reset,
        cursor: Some("cursor".into()),
        reason: None,
    };
    assert!(encode_result(&invalid, "0.0.10").is_err());
}
