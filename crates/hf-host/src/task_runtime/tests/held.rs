//! What an owner holds in retention, and the dispatcher-side hooks.
use super::{ctx::Ctx, harness::deadline, probe::Probe};
use crate::task_runtime::{
    owner_runtime::Runtime,
    seams::{self},
};
use hf_platform::backend_launch::observe_selection;
use std::{any::Any, ffi::OsString, rc::Rc};

/// What an owner in retention holds, read by the observer.
#[derive(Debug)]
pub(super) struct Held {
    pub(super) phase: hf_domain::ownership::Phase,
    pub(super) revision: u64,
    pub(super) task: Option<String>,
    pub(super) agent: bool,
    pub(super) reader: bool,
    pub(super) writer: bool,
    pub(super) discovery: bool,
}
pub(super) fn held(runtime: &Runtime, probe: &Probe) -> Held {
    let evidence = probe.evidence();
    Held {
        phase: evidence.ownership.phase,
        revision: evidence.revision,
        task: probe.task_status(),
        agent: runtime.agent.is_some(),
        reader: runtime.reader.is_some(),
        writer: runtime.writer.is_some(),
        discovery: runtime.discovery.is_some(),
    }
}
/// The state of a root that has a dispatcher-written T0 and nothing else.
pub(super) fn request(ctx: &Ctx) -> hf_application::ctl::task_run::RunRequest {
    hf_application::ctl::task_run::RunRequest {
        backend: "claude".into(),
        freedom: "unrestricted".into(),
        repo: std::fs::canonicalize(std::env::current_dir().unwrap())
            .unwrap()
            .to_str()
            .unwrap()
            .into(),
        prompt: format!("Run the scripted task in {}.", ctx.dir.display()),
    }
}
/// The dispatcher-side hooks: the scripted caller and the fake as the selected backend.
pub(super) fn dispatcher_hooks(
    ctx: &Ctx,
    fault: Option<Rc<seams::DispatchHook>>,
) -> Vec<Box<dyn Any>> {
    let env = ctx
        .fake
        .environment("read", hf_backends::backend_launch::CLAUDE_VERSION);
    let env_os: Vec<_> = env
        .iter()
        .map(|(k, v)| (OsString::from(k), OsString::from(v)))
        .collect();
    let pin = observe_selection(&env_os, &hf_storage::Sha256Digest, deadline()).unwrap();
    let mut hooks: Vec<Box<dyn Any>> = vec![
        Box::new(seams::install_caller(ctx.scripted.hook())),
        Box::new(seams::install_pin(Rc::new(move |_| Ok(pin.clone())))),
    ];
    if let Some(fault) = fault {
        hooks.push(Box::new(seams::install_dispatch(fault)));
    }
    hooks
}
