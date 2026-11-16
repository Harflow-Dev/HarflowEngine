#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]
//! Recovery result and qualified failure documents, and the dispatch service boundary.
use hf_application::ctl::state_recover::{
    flow::{RecoveryError, RecoveryFailure, RecoveryPhase, RecoveryProgress},
    journal::RecoveryResult,
};
use hf_domain::ctl::{
    identity::{HostIdentity, PRODUCT_VERSION},
    inspect::{InspectReason, InspectStatus, StateInspection},
    state_root::{InvalidStateRoot, StateRoot},
};
use hf_interfaces::ctl::dispatch::{CtlServices, dispatch};
use serde_json::Value;
use std::cell::Cell;

const ROOT: &str = "/tmp/harflow-state";
const OPERATION: &str = "0123456789abcdef0123456789abcdef";
const BACKUP: &str = "/tmp/harflow-state.harflow-backup-0123456789abcdef0123456789abcdef";

struct Services {
    resolves: Cell<usize>,
    recover_calls: Cell<usize>,
    recovery: Result<RecoveryResult, RecoveryFailure>,
}

impl Services {
    fn with_recovery(recovery: Result<RecoveryResult, RecoveryFailure>) -> Self {
        Self {
            resolves: Cell::new(0),
            recover_calls: Cell::new(0),
            recovery,
        }
    }
}

impl CtlServices for Services {
    fn resolve_state_root(&self) -> Result<StateRoot, InvalidStateRoot> {
        self.resolves.set(self.resolves.get() + 1);
        Ok(StateRoot::from_normalized(ROOT.into()))
    }

    fn host_identity(&self) -> HostIdentity {
        HostIdentity::from_rust("macos", "aarch64")
    }

    fn inspect(&self, root: &StateRoot) -> StateInspection {
        StateInspection::new(
            root,
            InspectStatus::Ready,
            InspectReason::Compatible,
            vec![],
        )
    }

    fn store_reads_supported(&self) -> bool {
        true
    }

    fn recover(
        &self,
        root: &StateRoot,
        operation_id: &str,
        confirmation_token: &str,
    ) -> Result<RecoveryResult, RecoveryFailure> {
        self.recover_calls.set(self.recover_calls.get() + 1);
        assert_eq!(root.as_str(), ROOT);
        assert_eq!(operation_id, OPERATION);
        assert_eq!(confirmation_token, "opaque-confirmation-token");
        self.recovery.clone()
    }
}

fn args(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| (*value).to_string()).collect()
}

fn result() -> RecoveryResult {
    RecoveryResult {
        operation_id: OPERATION.into(),
        state_root: ROOT.into(),
        backup_path: BACKUP.into(),
    }
}

fn progress(phase: RecoveryPhase) -> RecoveryFailure {
    RecoveryFailure {
        error: RecoveryError::RecoveryInterrupted,
        progress: Some(RecoveryProgress {
            operation_id: OPERATION.into(),
            backup_path: BACKUP.into(),
            phase,
        }),
    }
}

fn keys(value: &Value) -> Vec<&str> {
    let mut found: Vec<_> = value
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    found.sort_unstable();
    found
}

#[test]
fn malformed_arguments_never_reach_the_recovery_service() {
    let services = Services::with_recovery(Ok(result()));
    let outcome = dispatch(
        &args(&["state-recover", "--operation-id", OPERATION]),
        &services,
    );
    assert_eq!(outcome.code, 2);
    assert!(outcome.stderr.contains("--confirmation-token"));
    assert_eq!(services.recover_calls.get(), 0);
}

#[test]
fn success_returns_the_canonical_result_keys_and_exit_code() {
    let services = Services::with_recovery(Ok(result()));
    let outcome = dispatch(
        &args(&[
            "state-recover",
            "--operation-id",
            OPERATION,
            "--confirmation-token",
            "opaque-confirmation-token",
            "--json",
        ]),
        &services,
    );
    assert_eq!((outcome.code, outcome.stderr.as_str()), (0, ""));
    assert_eq!(services.recover_calls.get(), 1);
    let document: Value = serde_json::from_str(&outcome.stdout).unwrap();
    assert_eq!(
        keys(&document),
        ["format", "product", "product_version", "result", "v"]
    );
    assert_eq!(document["format"], "harflow.ctl");
    assert_eq!(document["product"], "harflow");
    let result = &document["result"];
    assert_eq!(
        keys(result),
        [
            "backup_id",
            "backup_path",
            "engine_version",
            "operation_id",
            "phase",
            "recovery_supported",
            "state_root",
            "status",
        ]
    );
    assert_eq!(result["backup_id"], OPERATION);
    assert_eq!(result["backup_path"], BACKUP);
    assert_eq!(result["engine_version"], PRODUCT_VERSION);
    assert_eq!(result["phase"], "completed");
    assert_eq!(result["recovery_supported"], true);
    assert_eq!(result["state_root"], ROOT);
}

#[test]
fn qualified_failure_includes_exact_progress_and_provenance_fields() {
    for (phase, text) in [
        (RecoveryPhase::Planned, "planned"),
        (RecoveryPhase::Prepared, "prepared"),
        (RecoveryPhase::BackedUp, "backed_up"),
        (RecoveryPhase::Committed, "committed"),
        (RecoveryPhase::Completed, "completed"),
    ] {
        let failure = progress(phase);
        let root = StateRoot::from_normalized(ROOT.into());
        let error = hf_interfaces::ctl::recover_dispatch::render_failure(&root, &failure);
        assert_eq!(keys(&error), ["code", "data", "message"]);
        assert_eq!(error["code"], "state_recovery_failed");
        let data = &error["data"];
        assert_eq!(
            keys(data),
            [
                "backup_id",
                "backup_path",
                "engine_version",
                "operation_id",
                "phase",
                "reason",
                "recovery_supported",
                "state_root",
            ]
        );
        assert_eq!(data["backup_id"], OPERATION);
        assert_eq!(data["backup_path"], BACKUP);
        assert_eq!(data["engine_version"], PRODUCT_VERSION);
        assert_eq!(data["operation_id"], OPERATION);
        assert_eq!(data["phase"], text);
        assert_eq!(data["reason"], "recovery_interrupted");
        assert_eq!(data["recovery_supported"], true);
        assert_eq!(data["state_root"], ROOT);
    }
}

#[test]
fn json_failure_envelope_has_one_error_with_qualified_progress() {
    let services = Services::with_recovery(Err(progress(RecoveryPhase::BackedUp)));
    let outcome = dispatch(
        &args(&[
            "state-recover",
            "--operation-id",
            OPERATION,
            "--confirmation-token",
            "opaque-confirmation-token",
            "--json",
        ]),
        &services,
    );
    assert_eq!(outcome.code, 1);
    assert_eq!(
        outcome.stderr,
        "harflow-ctl: error: State recovery did not complete\n"
    );
    let document: Value = serde_json::from_str(&outcome.stdout).unwrap();
    assert!(document.get("result").is_none());
    assert_eq!(
        keys(&document),
        ["error", "format", "product", "product_version", "v"]
    );
    assert_eq!(document["format"], "harflow.ctl");
    assert_eq!(document["product"], "harflow");
    let error = &document["error"];
    assert_eq!(keys(error), ["code", "data", "message"]);
    assert_eq!(error["message"], "State recovery did not complete");
    assert_eq!(error["data"]["phase"], "backed_up");
    assert_eq!(error["data"]["reason"], "recovery_interrupted");
    assert_eq!(services.recover_calls.get(), 1);
}

#[test]
fn unsupported_layout_preserves_the_layout_two_refusal_without_progress() {
    let root = StateRoot::from_normalized(ROOT.into());
    let error = hf_interfaces::ctl::recover_dispatch::render_failure(
        &root,
        &RecoveryError::UnsupportedLayout.into(),
    );
    assert_eq!(error["code"], "state_recovery_unsafe");
    assert_eq!(error["data"]["reason"], "phase_layout_recovery_unsupported");
    assert_eq!(error["data"]["recovery_supported"], false);
    assert!(error["data"].get("phase").is_none());
    assert!(error["data"].get("operation_id").is_none());
    assert!(error["data"].get("backup_path").is_none());
}
