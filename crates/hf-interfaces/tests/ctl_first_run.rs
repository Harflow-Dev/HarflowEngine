#![allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]

use hf_application::ctl::task_run::{RunFailure, RunRequest};
use hf_domain::ctl::{
    identity::HostIdentity,
    inspect::{InspectReason, InspectStatus, StateInspection},
    state_root::{InvalidStateRoot, StateRoot},
};
use hf_interfaces::ctl::dispatch::{CtlServices, Outcome, dispatch};
use serde_json::Value;
use std::cell::{Cell, RefCell};

const TASK: &str = "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa";
const WARNING: &str = "harflow-ctl: experimental run/status checkpoint; cancellation is unavailable; launch acceptance is not completion.\n";

struct Services {
    root_calls: Cell<usize>,
    run_calls: Cell<usize>,
    request: RefCell<Option<RunRequest>>,
    result: RefCell<Result<String, RunFailure>>,
    run_supported: bool,
}

impl Services {
    fn new(result: Result<String, RunFailure>) -> Self {
        Self {
            root_calls: Cell::new(0),
            run_calls: Cell::new(0),
            request: RefCell::new(None),
            result: RefCell::new(result),
            run_supported: true,
        }
    }

    fn without_run_support() -> Self {
        Self {
            run_supported: false,
            ..Self::new(Ok(TASK.into()))
        }
    }
}

impl CtlServices for Services {
    fn resolve_state_root(&self) -> Result<StateRoot, InvalidStateRoot> {
        self.root_calls.set(self.root_calls.get() + 1);
        Ok(StateRoot::from_normalized("/tmp/harflow-state".into()))
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
        self.run_supported
    }
    fn run_task(&self, _: &StateRoot, request: &RunRequest) -> Result<String, RunFailure> {
        self.run_calls.set(self.run_calls.get() + 1);
        *self.request.borrow_mut() = Some(request.clone());
        self.result.borrow().clone()
    }
}

fn run_args(prompt: &str) -> Vec<String> {
    [
        "run",
        "--backend",
        "claude",
        "--freedom",
        "unrestricted",
        "--repo",
        "/repo",
        "--prompt",
        prompt,
    ]
    .into_iter()
    .map(str::to_owned)
    .collect()
}

#[test]
fn malformed_run_syntax_and_cancel_never_resolve_state_or_call_executor() {
    let service = Services::new(Ok(TASK.into()));
    let mut duplicate = run_args("do work");
    duplicate.extend(["--backend".into(), "claude".into()]);
    assert_eq!(dispatch(&duplicate, &service).code, 2);
    assert_eq!(
        dispatch(
            &["run".into(), "--backend".into(), "claude".into()],
            &service
        )
        .code,
        2
    );
    let cancel = dispatch(&["cancel".into(), TASK.into(), "--json".into()], &service);
    assert_eq!(cancel.code, 1);
    assert_eq!(service.root_calls.get(), 0);
    assert_eq!(service.run_calls.get(), 0);
}

#[test]
fn unsupported_composition_does_not_expose_run_or_call_services() {
    let service = Services::without_run_support();
    let outcome = dispatch(&run_args("do work"), &service);
    assert_eq!(outcome.code, 2);
    assert!(outcome.stderr.contains("invalid choice: 'run'"));
    assert_eq!(service.root_calls.get(), 0);
    assert_eq!(service.run_calls.get(), 0);
}

#[test]
fn accepted_request_is_forwarded_once_and_success_keeps_warning_separate() {
    let service = Services::new(Ok(TASK.into()));
    let outcome = dispatch(&run_args("inspect and summarize"), &service);
    assert_eq!(
        outcome,
        Outcome {
            stdout: format!("{TASK}\n"),
            stderr: WARNING.into(),
            code: 0
        }
    );
    assert_eq!(service.root_calls.get(), 1);
    assert_eq!(service.run_calls.get(), 1);
    let request = service.request.borrow().clone().unwrap();
    assert_eq!(
        (
            request.backend.as_str(),
            request.freedom.as_str(),
            request.repo.as_str(),
            request.prompt.as_str()
        ),
        ("claude", "unrestricted", "/repo", "inspect and summarize")
    );

    let json_service = Services::new(Ok(TASK.into()));
    let mut args = run_args("inspect");
    args.push("--json".into());
    let json = dispatch(&args, &json_service);
    assert_eq!(json.stdout.lines().count(), 1);
    assert!(json.stdout.ends_with('\n'));
    let document: Value = serde_json::from_str(&json.stdout).unwrap();
    assert_eq!(document["result"]["task_id"], TASK);
    assert_eq!(document["format"], "harflow.ctl");
    let keys = document
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(
        keys,
        ["format", "product", "product_version", "result", "v"]
            .into_iter()
            .collect()
    );
    assert_eq!(json.stderr, WARNING);
}

#[test]
fn invalid_run_policy_and_prompt_are_refused_before_root_resolution() {
    for prompt in [
        String::new(),
        " \t\n".into(),
        "bad\0prompt".into(),
        "x".repeat(1_048_577),
    ] {
        let service = Services::new(Ok(TASK.into()));
        let outcome = dispatch(&run_args(&prompt), &service);
        assert_eq!(outcome.code, 1);
        assert_eq!(service.root_calls.get(), 0);
        assert_eq!(service.run_calls.get(), 0);
    }
    let service = Services::new(Ok(TASK.into()));
    let mut args = run_args("do work");
    let freedom = args.iter().position(|arg| arg == "unrestricted").unwrap();
    args[freedom] = "workspace-write".into();
    assert_eq!(dispatch(&args, &service).code, 1);
    assert_eq!(service.root_calls.get(), 0);
}

#[test]
fn unknown_backend_outcome_uses_unknown_envelope_and_exit_code() {
    let service = Services::new(Err(RunFailure::Unknown(
        "state changed during dispatch".into(),
    )));
    let mut args = run_args("do work");
    args.push("--json".into());
    let outcome = dispatch(&args, &service);
    assert_eq!(outcome.code, 3);
    assert!(outcome.stdout.contains("\"unknown\""));
    assert!(!outcome.stdout.contains("\"result\""));
    assert!(outcome.stderr.is_empty());
}
